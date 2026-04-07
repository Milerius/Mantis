//! Adapter for `mantis_queue::SpscRing` (move semantics, inline storage).

use crate::message::Message48;
use crate::queues::{QueueBench, QueueConsumer, QueueProducer};

use mantis_queue::{InlineStorage, spsc_ring};

type MantisProducer = mantis_queue::Producer<
    Message48,
    InlineStorage<Message48, 1024>,
    mantis_queue::Pow2Masked,
    mantis_queue::ImmediatePush,
    mantis_queue::NoInstr,
>;

type MantisConsumer = mantis_queue::Consumer<
    Message48,
    InlineStorage<Message48, 1024>,
    mantis_queue::Pow2Masked,
    mantis_queue::ImmediatePush,
    mantis_queue::NoInstr,
>;

pub struct MantisInlineQueue {
    tx: Option<MantisProducer>,
    rx: Option<MantisConsumer>,
}

impl MantisInlineQueue {
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = spsc_ring::<Message48, 1024>();
        Self {
            tx: Some(tx),
            rx: Some(rx),
        }
    }
}

impl QueueBench for MantisInlineQueue {
    type Producer = MantisInlineProducer;
    type Consumer = MantisInlineConsumer;

    fn name(&self) -> &'static str {
        "mantis-inline"
    }

    fn split(mut self) -> (Self::Producer, Self::Consumer) {
        (
            MantisInlineProducer(self.tx.take().expect("producer already taken")),
            MantisInlineConsumer(self.rx.take().expect("consumer already taken")),
        )
    }
}

pub struct MantisInlineProducer(MantisProducer);
pub struct MantisInlineConsumer(MantisConsumer);

impl QueueProducer for MantisInlineProducer {
    #[inline]
    fn try_push(&mut self, msg: &Message48) -> bool {
        self.0.try_push(*msg).is_ok()
    }
}

impl QueueConsumer for MantisInlineConsumer {
    #[inline]
    fn try_pop(&mut self, out: &mut Message48) -> bool {
        match self.0.try_pop() {
            Ok(val) => {
                *out = val;
                true
            }
            Err(_) => false,
        }
    }
}
