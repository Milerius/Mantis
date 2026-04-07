//! Adapter for `rtrb` crate.

use crate::message::Message48;
use crate::queues::{QueueBench, QueueConsumer, QueueProducer};

pub struct RtrbQueue {
    tx: Option<rtrb::Producer<Message48>>,
    rx: Option<rtrb::Consumer<Message48>>,
}

impl RtrbQueue {
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = rtrb::RingBuffer::new(1024);
        Self {
            tx: Some(tx),
            rx: Some(rx),
        }
    }
}

impl QueueBench for RtrbQueue {
    type Producer = RtrbProducer;
    type Consumer = RtrbConsumer;

    fn name(&self) -> &'static str {
        "rtrb"
    }

    fn split(mut self) -> (Self::Producer, Self::Consumer) {
        (
            RtrbProducer(self.tx.take().expect("producer already taken")),
            RtrbConsumer(self.rx.take().expect("consumer already taken")),
        )
    }
}

pub struct RtrbProducer(rtrb::Producer<Message48>);
pub struct RtrbConsumer(rtrb::Consumer<Message48>);

impl QueueProducer for RtrbProducer {
    #[inline]
    fn try_push(&mut self, msg: &Message48) -> bool {
        self.0.push(*msg).is_ok()
    }
}

impl QueueConsumer for RtrbConsumer {
    #[inline]
    fn try_pop(&mut self, out: &mut Message48) -> bool {
        match self.0.pop() {
            Ok(val) => {
                *out = val;
                true
            }
            Err(_) => false,
        }
    }
}
