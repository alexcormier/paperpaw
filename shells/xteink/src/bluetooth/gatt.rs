//! A link's GATT client: discovering a device's services, subscribing to
//! a characteristic, and reading a remote's HID reports.

use super::*;

/// The device's services (only `wanted`, where some are named), their
/// characteristics and descriptors.
pub(super) async fn discover(
    client: &Client<'_>,
    wanted: &[SdkUuid],
    known: &RefCell<Known>,
) -> Result<Vec<GattService>, BtError> {
    let mut handles: Vec<ServiceHandle> = Vec::new();
    if wanted.is_empty() {
        handles.extend(client.services().await.map_err(|e| error_of(&e))?);
    } else {
        for uuid in wanted {
            let found = client.services_by_uuid(&host_uuid(*uuid)).await;
            handles.extend(found.map_err(|e| error_of(&e))?);
        }
    }
    let mut services = Vec::new();
    for service in &handles {
        let found = client
            .characteristics::<CHARACTERISTICS>(service)
            .await
            .map_err(|e| error_of(&e))?;
        let mut characteristics = Vec::new();
        for c in &found {
            let properties = Properties::from_bits(c.props.to_raw());
            if let Some(cccd) = c.cccd_handle {
                let mut known = known.borrow_mut();
                known.configured.retain(|(h, _, _)| *h != c.handle);
                known.configured.push((c.handle, cccd, properties));
            }
            let mut characteristic =
                GattCharacteristic::new(sdk_uuid(&c.uuid), Handle::new(c.handle), properties);
            let descriptors = client
                .descriptors::<_, DESCRIPTORS>(c)
                .await
                .map_err(|e| error_of(&e))?;
            characteristic.descriptors = descriptors
                .iter()
                .map(|d| GattDescriptor::new(sdk_uuid(d.uuid()), Handle::new(d.handle())))
                .collect();
            characteristics.push(characteristic);
        }
        services.push(GattService::new(sdk_uuid(&service.uuid()), characteristics));
    }
    Ok(services)
}

/// Notifications (or indications, where the characteristic only
/// indicates) of `handle` on, or off.
pub(super) async fn subscribe(
    client: &Client<'_>,
    handle: u16,
    on: bool,
    known: &RefCell<Known>,
) -> Result<(), BtError> {
    let (cccd, properties) = known
        .borrow()
        .configured
        .iter()
        .find(|(h, _, _)| *h == handle)
        .map(|(_, cccd, properties)| (*cccd, *properties))
        .ok_or(BtError::NotFound)?;
    let value: u16 = match (on, properties.contains(Properties::NOTIFY)) {
        (false, _) => 0,
        (true, true) => 1,
        (true, false) => 2,
    };
    client
        .write_handle(cccd, &value.to_le_bytes())
        .await
        .map_err(|e| error_of(&e))
}

/// A remote's HID service: its report map told, its input reports
/// subscribed (or the boot keyboard's, where it has no reports).
pub(super) async fn hid(
    client: &Client<'_>,
    number: u32,
    known: &RefCell<Known>,
    shared: &Shared,
) -> Result<(), BtError> {
    // Met before: only its reports switched on again.
    let cached: Vec<u16> = known.borrow().hid.iter().map(|(h, _)| *h).collect();
    if !cached.is_empty() {
        let mut all = true;
        for handle in &cached {
            all &= subscribe(client, *handle, true, known).await.is_ok();
        }
        if all {
            log::info!("bluetooth: link {number}: HID as before");
            return Ok(());
        }
        log::info!("bluetooth: link {number}: HID changed, found anew");
        let address = known.borrow().address.clone();
        shared.hid.borrow_mut().retain(|h| h.address != address);
        known.borrow_mut().hid.clear();
    }
    let service = client
        .services_by_uuid(&Uuid::new_short(HID))
        .await
        .map_err(|e| error_of(&e))?
        .into_iter()
        .next()
        .ok_or(BtError::NotFound)?;
    let characteristics = client
        .characteristics::<CHARACTERISTICS>(&service)
        .await
        .map_err(|e| error_of(&e))?;
    let mut map_len = 0;
    let mut kept_map = Vec::new();
    if let Some(map) = characteristics
        .iter()
        .find(|c| c.uuid == Uuid::new_short(REPORT_MAP))
    {
        let mut buffer = alloc::vec![0; REPORT_MAP_MOST];
        let n = client
            .read_handle_blob(map.handle, 0, &mut buffer)
            .await
            .map_err(|e| error_of(&e))?;
        buffer.truncate(n);
        map_len = n;
        kept_map.clone_from(&buffer);
        shared.report(BtReport::HidMap {
            link: number,
            map: buffer,
        });
    }
    let mut inputs: Vec<(u16, u8)> = Vec::new();
    for c in characteristics
        .iter()
        .filter(|c| c.uuid == Uuid::new_short(REPORT) && c.cccd_handle.is_some())
    {
        let reference = client
            .descriptor_by_uuid::<_, [u8]>(c, &Uuid::new_short(REPORT_REFERENCE))
            .await;
        let mut id_and_type = [0, 1];
        if let Ok(reference) = reference {
            let _ = client.read_descriptor(&reference, &mut id_and_type).await;
        }
        // Type 1: an input report.
        if id_and_type[1] == 1 {
            inputs.push((c.handle, id_and_type[0]));
        }
    }
    if inputs.is_empty()
        && let Some(boot) = characteristics
            .iter()
            .find(|c| c.uuid == Uuid::new_short(BOOT_KEYBOARD_INPUT))
    {
        inputs.push((boot.handle, 0));
    }
    if inputs.is_empty() {
        return Err(BtError::NotFound);
    }
    for c in characteristics.iter() {
        if let Some(cccd) = c.cccd_handle {
            let properties = Properties::from_bits(c.props.to_raw());
            let mut known = known.borrow_mut();
            known.configured.retain(|(h, _, _)| *h != c.handle);
            known.configured.push((c.handle, cccd, properties));
        }
    }
    for (handle, _) in &inputs {
        subscribe(client, *handle, true, known).await?;
    }
    log::info!(
        "bluetooth: link {number}: HID reports {inputs:?}, map {} bytes",
        map_len
    );
    let address = known.borrow().address.clone();
    let configured = known.borrow().configured.clone();
    let mut cache = shared.hid.borrow_mut();
    cache.retain(|h| h.address != address);
    cache.push(HidCache {
        address,
        inputs: inputs.clone(),
        configured,
        map: kept_map,
    });
    drop(cache);
    known.borrow_mut().hid = inputs;
    Ok(())
}
