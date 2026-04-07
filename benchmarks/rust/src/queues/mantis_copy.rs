//! Adapter for `mantis_queue::SpscRingCopy` (copy-optimized, inline storage).

use crate::message::Message48;
use crate::queues::{QueueBench, QueueConsumer, QueueProducer};

use mantis_queue::{InlineStorage, spsc_ring_copy};

type MantisCopyProducerInner = mantis_queue::ProducerCopy<
    Message48,
    InlineStorage<Message48, 1024>,
    mantis_queue::Pow2Masked,
    mantis_queue::ImmediatePush,
    mantis_queue::NoInstr,
    mantis_platform::DefaultCopyPolicy,
>;

type MantisCopyConsumerInner = mantis_queue::ConsumerCopy<
    Message48,
    InlineStorage<Message48, 1024>,
    mantis_queue::Pow2Masked,
    mantis_queue::ImmediatePush,
    mantis_queue::NoInstr,
    mantis_platform::DefaultCopyPolicy,
>;

pub struct MantisCopyQueue {
    tx: Option<MantisCopyProducerInner>,
    rx: Option<MantisCopyConsumerInner>,
}

impl MantisCopyQueue {
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = spsc_ring_copy::<Message48, 1024>();
        Self {
            tx: Some(tx),
            rx: Some(rx),
        }
    }
}

impl QueueBench for MantisCopyQueue {
    type Producer = MantisCopyProducer;
    type Consumer = MantisCopyConsumer;

    fn name(&self) -> &'static str {
        "mantis-copy"
    }

    fn split(mut self) -> (Self::Producer, Self::Consumer) {
        (
            MantisCopyProducer(self.tx.take().expect("producer already taken")),
            MantisCopyConsumer(self.rx.take().expect("consumer already taken")),
        )
    }
}

pub struct MantisCopyProducer(MantisCopyProducerInner);
pub struct MantisCopyConsumer(MantisCopyConsumerInner);

impl QueueProducer for MantisCopyProducer {
    #[inline]
    fn try_push(&mut self, msg: &Message48) -> bool {
        self.0.push(msg)
    }
}

impl QueueConsumer for MantisCopyConsumer {
    #[inline]
    fn try_pop(&mut self, out: &mut Message48) -> bool {
        self.0.pop(out)
    }
}
