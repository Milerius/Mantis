//! Queue abstraction for benchmarking.

pub mod mantis_copy;
pub mod mantis_inline;
pub mod rtrb_queue;

use crate::message::Message48;

/// A queue that can be split into producer/consumer handles for two-thread benchmarking.
pub trait QueueBench: Send {
    type Producer: QueueProducer + Send;
    type Consumer: QueueConsumer + Send;

    fn name(&self) -> &'static str;
    fn split(self) -> (Self::Producer, Self::Consumer);
}

pub trait QueueProducer {
    /// Try to push a message. Returns true on success, false if full.
    fn try_push(&mut self, msg: &Message48) -> bool;
}

pub trait QueueConsumer {
    /// Try to pop a message. Returns true on success (writes into `out`), false if empty.
    fn try_pop(&mut self, out: &mut Message48) -> bool;
}
