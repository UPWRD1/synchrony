use std::borrow::Cow;

use crate::{
    engine::{SlotIndex, bbp::PoolExecutor, tick::Tick},
    model::{
        DataKind,
        flow::{Node, Socket},
        project::ProjectData,
    },
};

#[derive(Debug, Clone)]
pub struct Master;

impl Master {
    const INPUTS: &'static [Socket] = &[Socket {
        kind: DataKind::Audio,
        name: "input",
        visible: true,
    }];

    const OUTPUTS: &'static [Socket] = &[Socket {
        kind: DataKind::Audio,
        name: "output",
        visible: false,
    }];
}

impl Node for Master {
    type State = ();

    fn inputs(&self) -> Cow<'_, [Socket]> {
        Cow::Borrowed(Self::INPUTS)
    }

    fn outputs(&self) -> Cow<'_, [Socket]> {
        Cow::Borrowed(Self::OUTPUTS)
    }

    fn init_state(&self) -> Self::State {}

    fn process(
        &self,
        pool: &mut PoolExecutor,
        _: &mut Self::State,
        _: &ProjectData,
        _: Tick,
        inputs: &[SlotIndex],
        outputs: &[SlotIndex],
    ) {
        let input_buf = pool.get_input(inputs[0]);
        let output_buf = pool.get_output(outputs[0]);

        output_buf.copy_from_slice(input_buf);
    }
}
