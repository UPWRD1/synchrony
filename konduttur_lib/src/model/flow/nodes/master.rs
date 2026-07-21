use crate::{
    engine::{SlotIndex, bbp::PoolExecutor, engineconfig::EngineConfig, tick::Tick},
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
    fn inputs(&self) -> &'static [Socket] {
        Self::INPUTS
    }

    fn outputs(&self) -> &'static [Socket] {
        Self::OUTPUTS
    }
    fn process(
        &self,
        _: &ProjectData,
        pool: &mut PoolExecutor,
        _: Tick,
        _: &EngineConfig,
        inputs: &[SlotIndex],
        outputs: &[SlotIndex],
    ) {
        let input_buf = pool.get_input(inputs[0]);
        let output_buf = pool.get_output(outputs[0]);

        output_buf.copy_from_slice(input_buf);
    }
}
