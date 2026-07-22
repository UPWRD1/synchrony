use std::{borrow::Cow, marker::PhantomData};

use crate::{
    engine::{SlotIndex, bbp::PoolExecutor, tick::Tick},
    model::{
        Audio, DataKind, Kind,
        flow::{Node, Socket},
        project::ProjectData,
    },
};

#[derive(Debug, Clone)]
pub struct Sum<K: Kind> {
    kind: PhantomData<K>,
    cached_inputs: Vec<Socket>,
}

impl<K: Kind> Sum<K> {
    pub fn new() -> Self {
        Self {
            kind: PhantomData,
            cached_inputs: vec![],
        }
    }
}

impl Sum<Audio> {
    const OUTPUTS: &'static [Socket] = &[Socket {
        name: "audio out",
        kind: DataKind::Audio,
        visible: true,
    }];
}

pub struct SumState {}

impl Node for Sum<Audio> {
    type State = SumState;

    fn init_state(&self) -> Self::State {
        SumState {}
    }

    fn process(
        &self,
        pool: &mut PoolExecutor,
        _state: &mut Self::State,
        _: &ProjectData,
        _: Tick,
        inputs: &[SlotIndex],
        outputs: &[SlotIndex],
    ) {
        let output_buf = pool.get_output(outputs[0]);
        for input_index in inputs {
            let source = pool.get_input(*input_index);
            // Simple additive loop: easily optimized by hardware auto-vectorization
            for sample_idx in 0..pool.block_size {
                output_buf[sample_idx] += source[sample_idx];
            }
        }
    }

    fn inputs(&self) -> Cow<'_, [Socket]> {
        Cow::Borrowed(&self.cached_inputs)
    }

    fn input(&mut self, idx: crate::model::flow::SocketIndex) -> Option<&Socket> {
        match idx {
            i if i < self.cached_inputs.len() => self.cached_inputs.get(i),
            i if i == self.cached_inputs.len() => {
                let s = Socket {
                    kind: DataKind::Audio,
                    name: format!("Input {i}").leak(),
                    visible: true,
                };
                self.cached_inputs.push(s);
                Some(&self.cached_inputs[i])
            }
            _ => None,
        }
    }

    fn outputs(&self) -> Cow<'_, [Socket]> {
        Cow::Borrowed(Self::OUTPUTS)
    }
}
