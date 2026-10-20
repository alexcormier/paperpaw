//! The OTA data partition's two entries, as the bootloader reads them:
//! the one with the higher sequence number chooses the app partition
//! started, and says in what state its image is.
//!
//! Choosing another partition (esp-bootloader-esp-idf's
//! `activate_next_partition`) writes a new sequence number into the
//! other entry and keeps the state that entry had, often "valid" from an
//! older update; the state of the new image is written apart, a second
//! time. A power cut in between would start the update as good, without
//! a trial. So the firmware marks that entry's state as new first
//! ([`next_entry`], [`mark_new`]): the bootloader does not look at the
//! entry not in force.

/// Where the two entries are in the partition: a flash sector each.
pub const ENTRIES: [u32; 2] = [0, 0x1000];
/// An entry's length: its sequence number, a label, its state and the
/// CRC of its sequence number.
pub const ENTRY_LEN: usize = 32;
/// The sequence number of an entry never written.
const UNWRITTEN: u32 = u32::MAX;
/// The state of an image not started yet (ESP-IDF's `ESP_OTA_IMG_NEW`).
const NEW: u32 = 0;
/// Where an entry's state is.
const STATE: core::ops::Range<usize> = 24..28;

/// An entry's sequence number.
fn sequence(entry: &[u8; ENTRY_LEN]) -> u32 {
    u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]])
}

/// The entry that choosing another app partition writes: the one not in
/// force. `None` when that entry was never written: its state is then
/// "undefined" and must stay so (with any other state it would be taken
/// as damaged), and the firmware takes an undefined state for a new
/// image.
pub fn next_entry(entries: &[[u8; ENTRY_LEN]; 2]) -> Option<usize> {
    let (first, second) = (sequence(&entries[0]), sequence(&entries[1]));
    // As the bootloader: the higher sequence number is in force, one
    // never written is not.
    let in_force = match (first, second) {
        (UNWRITTEN, UNWRITTEN) => None,
        (UNWRITTEN, _) => Some(1),
        (_, UNWRITTEN) => Some(0),
        _ if first > second => Some(0),
        _ => Some(1),
    };
    let next = match in_force {
        Some(0) => 1,
        _ => 0,
    };
    (sequence(&entries[next]) != UNWRITTEN).then_some(next)
}

/// Marks `entry`'s image as new (its sequence number and CRC stay).
pub fn mark_new(entry: &mut [u8; ENTRY_LEN]) {
    entry[STATE].copy_from_slice(&NEW.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An entry with this sequence number and state (the CRC does not
    /// matter here).
    fn entry(sequence: u32, state: u32) -> [u8; ENTRY_LEN] {
        let mut entry = [0xFF; ENTRY_LEN];
        entry[..4].copy_from_slice(&sequence.to_le_bytes());
        entry[STATE].copy_from_slice(&state.to_le_bytes());
        entry
    }

    const VALID: u32 = 2;
    const UNDEFINED: u32 = u32::MAX;

    #[test]
    fn the_entry_not_in_force_is_written_next() {
        let older = entry(3, VALID);
        let newer = entry(4, VALID);
        assert_eq!(next_entry(&[newer, older]), Some(1));
        assert_eq!(next_entry(&[older, newer]), Some(0));
    }

    #[test]
    fn an_entry_never_written_is_left_alone() {
        let unwritten = entry(UNWRITTEN, UNDEFINED);
        // Never chosen: the first choice writes the first entry.
        assert_eq!(next_entry(&[unwritten, unwritten]), None);
        // One chosen: the other is next, and was never written.
        assert_eq!(next_entry(&[entry(1, VALID), unwritten]), None);
        // The second written alone: the first is next.
        assert_eq!(next_entry(&[unwritten, entry(1, VALID)]), None);
    }

    #[test]
    fn marking_changes_the_state_alone() {
        let mut marked = entry(7, VALID);
        let before = marked;
        mark_new(&mut marked);
        assert_eq!(marked[STATE], NEW.to_le_bytes());
        assert_eq!(marked[..24], before[..24]);
        assert_eq!(marked[28..], before[28..]);
    }
}
