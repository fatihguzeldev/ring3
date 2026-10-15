use super::*;
use crate::graphics::d3d8::{
    resources::{BufferDesc, BufferKind, Pool, ResourceDesc, SubresourceInfo, Usage},
    state::DeviceState,
};

#[test]
fn create_upload_draw_versions_preserve_order_when_fragment_starts_after_existing_record() {
    let mut queue = queue(256);
    let id = resource(1).id;
    let info = SubresourceInfo {
        width: 96,
        height: 1,
        depth: 1,
        row_pitch: 96,
        slice_pitch: 96,
        length: 96,
        face: 0,
        level: 0,
    };
    queue
        .submit(&Command::CreateResource {
            id,
            desc: ResourceDesc::Buffer(BufferDesc {
                length: 96,
                kind: BufferKind::Vertex { fvf: 2 },
                usage: Usage(0),
                pool: Pool::Default,
            }),
            subresources: &[info],
        })
        .unwrap();
    let data = [7; 96];
    assert_eq!(
        queue.submit(&Command::Upload(Upload {
            resource: resource(2),
            subresource: 0,
            offset: 0,
            row_pitch: 96,
            slice_pitch: 96,
            row_bytes: 96,
            rows: 1,
            slices: 1,
            bytes: &data
        })),
        Ok(Progress::Draining)
    );
    let mut records = Vec::new();
    while let Some((id, bytes)) = queue.peek() {
        for fragment in packet(bytes).unwrap().1 {
            records.push((
                fragment.command,
                fragment.opcode,
                fragment.offset,
                fragment.flags,
            ));
        }
        queue.acknowledge(id).unwrap();
    }
    assert_eq!(records[0], (1, 1, 0, 3));
    assert_eq!(records[1], (2, 3, 0, 1));
    assert_eq!(records.last().unwrap().0, 2);
    assert_eq!(records.last().unwrap().3, 2);
    queue
        .submit(&Command::DestroyResource(resource(2)))
        .unwrap();
    queue.seal().unwrap();
    let record = packet(queue.peek().unwrap().1).unwrap().1.next().unwrap();
    assert_eq!((record.command, record.opcode), (3, 2));
    assert_eq!(
        u64::from_le_bytes(record.bytes[16..24].try_into().unwrap()),
        2
    );
}

#[test]
fn pipeline_bytes_are_immutable_and_guest_pointer_bindings_do_not_enter_snapshot() {
    let mut state = DeviceState::new(640, 480, false).unwrap();
    let mut before = Vec::new();
    crate::abi::graphics::d3d8::append_payload(
        &Command::Pipeline {
            id: PipelineId(1),
            state: state.snapshot(),
        },
        &mut before,
    )
    .unwrap();
    state.set_texture(0, 0x1234_5678).unwrap();
    state
        .set_stream(
            0,
            crate::graphics::d3d8::state::StreamBinding {
                resource: 0x3456_7890,
                stride: 20,
            },
        )
        .unwrap();
    let mut after = Vec::new();
    crate::abi::graphics::d3d8::append_payload(
        &Command::Pipeline {
            id: PipelineId(1),
            state: state.snapshot(),
        },
        &mut after,
    )
    .unwrap();
    assert_eq!(before, after);
    let mut queue = queue(64 * 1024);
    queue
        .submit(&Command::Pipeline {
            id: PipelineId(1),
            state: state.snapshot(),
        })
        .unwrap();
    state.set_render_state(7, 1).unwrap();
    queue.seal().unwrap();
    let record = packet(queue.peek().unwrap().1).unwrap().1.next().unwrap();
    assert_eq!(record.bytes, before);
}
