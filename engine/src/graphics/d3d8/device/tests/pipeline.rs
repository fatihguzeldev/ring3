use super::*;
use crate::graphics::{
    ResourceId,
    d3d8::{
        commands::{IndexBinding, TextureBinding},
        resources::{TextureDesc, TextureKind},
        state::{Matrix, StateBlockKind, StreamBinding},
    },
};

fn commands(device: &mut Device) -> Vec<(u16, Vec<u8>)> {
    let id = device.seal_batch().unwrap().unwrap();
    let (_, bytes) = device.queue().peek().unwrap();
    let (_, fragments) = crate::abi::graphics::d3d8::packet(bytes).unwrap();
    let result = fragments
        .map(|fragment| (fragment.opcode, fragment.bytes.to_vec()))
        .collect();
    device.acknowledge_batch(id).unwrap();
    result
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn identity(bytes: &[u8], at: usize) -> ResourceId {
    ResourceId {
        epoch: u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()),
        slot: word(bytes, at + 8),
        generation: word(bytes, at + 12),
    }
}

fn resources(device: &mut Device) -> [DrawBindings; 2] {
    std::array::from_fn(|index| {
        let texture = device
            .create_resource(ResourceDesc::Texture(TextureDesc {
                kind: TextureKind::Texture2D,
                width: 1,
                height: 1,
                depth: 1,
                levels: 1,
                format: Format::A8R8G8B8,
                usage: Usage(0),
                pool: Pool::Managed,
            }))
            .unwrap()
            .value;
        let vertex = device
            .create_resource(buffer(48, 2, Pool::Managed))
            .unwrap()
            .value;
        let indices = device
            .create_resource(ResourceDesc::Buffer(BufferDesc {
                length: 6,
                kind: BufferKind::Index {
                    format: Format::Index16,
                },
                usage: Usage(0),
                pool: Pool::Managed,
            }))
            .unwrap()
            .value;
        let mut bindings = DrawBindings::default();
        bindings.textures[7] = Some(TextureBinding {
            resource: device.resource_version(texture).unwrap(),
            lod: 0,
        });
        bindings.streams[0] = Some(StreamResource {
            resource: device.resource_version(vertex).unwrap(),
            stride: 12 + index as u32 * 4,
        });
        bindings.indices = Some(IndexBinding {
            resource: device.resource_version(indices).unwrap(),
            format: Format::Index16,
            base_vertex: index as u32,
        });
        bindings
    })
}

fn bind(device: &mut Device, bindings: DrawBindings) {
    let state = device.state_mut().unwrap();
    state
        .set_texture(7, bindings.textures[7].unwrap().resource.id.slot)
        .unwrap();
    state
        .set_stream(
            0,
            StreamBinding {
                resource: bindings.streams[0].unwrap().resource.id.slot,
                stride: bindings.streams[0].unwrap().stride,
            },
        )
        .unwrap();
    let indices = bindings.indices.unwrap();
    state
        .set_indices(indices.resource.id.slot, indices.base_vertex)
        .unwrap();
}

fn draw(device: &mut Device, bindings: DrawBindings) {
    device
        .draw(
            Primitive::PointList,
            1,
            bindings,
            DrawData::Vertices { first_vertex: 0 },
        )
        .unwrap();
}

#[test]
fn binding_only_draws_reuse_pipeline_and_encode_fresh_texture_vertex_and_index_bindings() {
    let mut device = device();
    device.bind_vertex_shader(2).unwrap();
    let bindings = resources(&mut device);
    drain(&mut device);
    device.begin_scene().unwrap();
    for binding in bindings {
        bind(&mut device, binding);
        draw(&mut device, binding);
    }
    let records = commands(&mut device);
    assert_eq!(
        records.iter().map(|record| record.0).collect::<Vec<_>>(),
        [6, 8, 8]
    );
    for (payload, binding) in records
        .iter()
        .filter(|record| record.0 == 8)
        .map(|record| &record.1)
        .zip(bindings)
    {
        assert_eq!(u64::from_le_bytes(payload[..8].try_into().unwrap()), 1);
        // the color target has no depth; seven empty texture slots precede slot seven.
        assert_eq!(word(payload, 48), 0);
        assert_eq!(word(payload, 80), 1);
        assert_eq!(
            identity(payload, 84),
            binding.textures[7].unwrap().resource.id
        );
        assert_eq!(word(payload, 112), 1);
        assert_eq!(
            identity(payload, 116),
            binding.streams[0].unwrap().resource.id
        );
        assert_eq!(word(payload, 140), binding.streams[0].unwrap().stride);
        assert_eq!(word(payload, 204), 1);
        assert_eq!(identity(payload, 208), binding.indices.unwrap().resource.id);
        assert_eq!(word(payload, 236), binding.indices.unwrap().base_vertex);
    }
}

#[test]
fn render_transform_and_constant_changes_each_publish_a_new_pipeline() {
    let mut device = device();
    device.bind_vertex_shader(2).unwrap();
    device.begin_scene().unwrap();
    let user_draw = |device: &mut Device| {
        device
            .draw(
                Primitive::PointList,
                1,
                DrawBindings::default(),
                DrawData::UserVertices {
                    stride: 12,
                    vertices: &[0; 12],
                },
            )
            .unwrap();
    };
    user_draw(&mut device);
    device.state_mut().unwrap().set_render_state(22, 1).unwrap();
    user_draw(&mut device);
    let mut transform = Matrix::IDENTITY;
    transform.0[12] = 1.0f32.to_bits();
    device
        .state_mut()
        .unwrap()
        .set_transform(256, transform)
        .unwrap();
    user_draw(&mut device);
    device
        .state_mut()
        .unwrap()
        .set_vertex_constants(95, &[[1, 2, 3, 4]])
        .unwrap();
    user_draw(&mut device);
    let records = commands(&mut device);
    assert_eq!(
        records.iter().map(|record| record.0).collect::<Vec<_>>(),
        [6, 8, 6, 8, 6, 8, 6, 8]
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.0 == 6)
            .map(|record| u64::from_le_bytes(record.1[..8].try_into().unwrap()))
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
}

#[test]
fn binding_only_state_blocks_preserve_pipeline_but_mixed_blocks_invalidate_it() {
    let mut device = device();
    device.bind_vertex_shader(2).unwrap();
    let bindings = resources(&mut device);
    drain(&mut device);
    bind(&mut device, bindings[0]);
    device.begin_scene().unwrap();
    draw(&mut device, bindings[0]);
    let revision = device.state().revision();
    device.begin_state_block().unwrap();
    bind(&mut device, bindings[1]);
    assert_eq!(device.state().revision(), revision);
    let block = device.end_state_block().unwrap();
    device.apply_state_block(block).unwrap();
    assert!(device.state().revision() > revision);
    draw(&mut device, bindings[1]);
    device.begin_state_block().unwrap();
    bind(&mut device, bindings[0]);
    device.state_mut().unwrap().set_render_state(22, 1).unwrap();
    let mixed = device.end_state_block().unwrap();
    device.apply_state_block(mixed).unwrap();
    draw(&mut device, bindings[0]);
    let records = commands(&mut device);
    assert_eq!(
        records.iter().map(|record| record.0).collect::<Vec<_>>(),
        [6, 8, 8, 6, 8]
    );
    let block = device.create_state_block(StateBlockKind::All).unwrap();
    device.apply_state_block(block).unwrap();
    draw(&mut device, bindings[0]);
    assert_eq!(
        commands(&mut device)
            .iter()
            .map(|record| record.0)
            .collect::<Vec<_>>(),
        [6, 8]
    );
}
