//! Handles of what a network stack made (sockets, name lookups), each
//! with the number of the stack that made it. The firmware makes its
//! stack anew whenever the radio starts anew (switched on, or restarted
//! after a connection stalled without an answer, see [`crate::stall`]),
//! and a handle kept from the stack before is not this one's: smoltcp
//! numbers sockets by their place in a list, so the old number names
//! another socket, or none (which panicked). The stack passes over a
//! handle of another number: what it named is gone with the old stack.

/// A handle made by the stack numbered `generation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tagged<H> {
    handle: H,
    generation: u32,
}

impl<H: Copy> Tagged<H> {
    /// `handle`, made by the stack numbered `generation`.
    pub fn new(handle: H, generation: u32) -> Self {
        Self { handle, generation }
    }

    /// The handle, if the stack numbered `generation` made it; `None` for
    /// one an earlier stack made.
    pub fn of(self, generation: u32) -> Option<H> {
        (self.generation == generation).then_some(self.handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use smoltcp::iface::{SocketHandle, SocketSet};
    use smoltcp::socket::udp;

    fn udp_socket() -> udp::Socket<'static> {
        let buffer = || udp::PacketBuffer::new(Vec::new(), Vec::new());
        udp::Socket::new(buffer(), buffer())
    }

    /// The stack before the radio started anew: DHCP's socket, the name
    /// servers', and the clock's question as the third.
    fn old_stack() -> (SocketSet<'static>, SocketHandle) {
        let mut sockets = SocketSet::new(Vec::new());
        sockets.add(udp_socket());
        sockets.add(udp_socket());
        let clock = sockets.add(udp_socket());
        (sockets, clock)
    }

    #[test]
    fn a_handle_of_the_stack_before_is_passed_over() {
        let (_, clock) = old_stack();
        let kept = Tagged::new(clock, 1);
        // The new stack holds DHCP's socket alone so far.
        let mut sockets = SocketSet::new(Vec::new());
        sockets.add(udp_socket());
        if let Some(handle) = kept.of(2) {
            sockets.remove(handle);
        }
        assert_eq!(sockets.iter().count(), 1);
    }

    #[test]
    fn a_handle_of_this_stack_is_its_own() {
        let (mut sockets, clock) = old_stack();
        let kept = Tagged::new(clock, 1);
        let handle = kept.of(1).expect("this stack's");
        sockets.remove(handle);
        assert_eq!(sockets.iter().count(), 2);
    }

    /// What happened without the number (seen on the X4 in the beta.8
    /// update test): the old handle let go on the new stack.
    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn without_the_number_the_new_stack_panicked() {
        let (_, clock) = old_stack();
        let mut sockets = SocketSet::new(Vec::new());
        sockets.add(udp_socket());
        sockets.remove(clock);
    }
}
