use super::*;
use crate::graphics::d3d8::{
    commands::{ShaderDefinition, Upload},
    shader::{Program, Stage},
};

#[test]
fn texture_lod_is_an_immutable_resolved_binding_in_each_job() {
    use crate::graphics::d3d8::commands::{DrawBindings, PipelineId, TextureBinding};
    let mut bindings = DrawBindings::default();
    bindings.textures[0] = Some(TextureBinding {
        resource: ResourceVersion {
            id: source().surface.resource,
            version: 2,
        },
        lod: 3,
    });
    let mut bytes = Vec::new();
    append_payload(
        &Command::ProcessVertices {
            pipeline: PipelineId(1),
            bindings,
            source_start: 0,
            destination: ResourceVersion {
                id: source().surface.resource,
                version: 3,
            },
            destination_index: 0,
            vertex_count: 1,
            flags: 0,
            destination_fvf: 4,
        },
        &mut bytes,
    )
    .unwrap();
    assert_eq!(
        &bytes[8..40],
        &[
            1, 0, 0, 0, 77, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 5, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 3,
            0, 0, 0
        ]
    );
}

#[test]
fn indexed_user_vertices_transport_only_the_selected_nonzero_origin_span() {
    use crate::graphics::d3d8::commands::{
        Draw, DrawBindings, DrawData, PipelineId, Primitive, Targets,
    };
    let vertices = [7; 12];
    let indices = [5, 0, 6, 0, 7, 0];
    fn make<'a>(vertices: &'a [u8], indices: &'a [u8]) -> Command<'a> {
        Command::Draw(Draw {
            pipeline: PipelineId(1),
            primitive: Primitive::TriangleList,
            primitive_count: 1,
            bindings: DrawBindings::default(),
            targets: Targets {
                color: source(),
                depth: None,
            },
            data: DrawData::UserIndexed {
                min_vertex: 5,
                vertex_count: 3,
                stride: 4,
                vertices,
                format: crate::graphics::d3d8::resources::Format::Index16,
                indices,
            },
        })
    }
    assert!(payload_size(&make(&vertices, &indices)).is_ok());
    assert_eq!(
        payload_size(&make(&[0; 32], &indices)),
        Err(CodecError::Length)
    );
}

#[test]
fn upload_rows_keep_explicit_pitch_and_exact_source_byte_order() {
    let source_bytes = [7, 8, 99, 99, 99, 99, 99, 99, 9, 10];
    let command = Command::Upload(Upload {
        resource: ResourceVersion {
            id: source().surface.resource,
            version: 2,
        },
        subresource: 3,
        offset: 18,
        row_pitch: 8,
        slice_pitch: 64,
        row_bytes: 2,
        rows: 2,
        slices: 1,
        bytes: &source_bytes,
    });
    assert_eq!(payload_size(&command), Ok((3, 66)));
    let mut bytes = Vec::new();
    append_payload(&command, &mut bytes).unwrap();
    assert_eq!(
        &bytes[..24],
        &[
            77, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 5, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0
        ]
    );
    let words: Vec<u32> = bytes[24..56]
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect();
    assert_eq!(words, [3, 18, 8, 64, 2, 2, 1, 10]);
    assert_eq!(&bytes[56..], &source_bytes);
}

#[test]
fn shader_def_is_bind_metadata_and_cannot_be_reexecuted_as_draw_instruction() {
    let program = Program::parse(
        &[
            0xffff_0101,
            81,
            0xa00f_0002,
            0x3f80_0000,
            0,
            0,
            0x3f80_0000,
            1,
            0x800f_0000,
            0xa0e4_0002,
            0xffff,
        ],
        Stage::Pixel,
    )
    .unwrap();
    let mut bytes = Vec::new();
    append_payload(
        &Command::CreateShader {
            handle: 9,
            definition: ShaderDefinition::Pixel { program: &program },
        },
        &mut bytes,
    )
    .unwrap();
    let words: Vec<u32> = bytes
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect();
    assert_eq!(&words[..6], &[9, 2, 2, 1, 1, 2]);
    assert_eq!(&words[6..10], &[0x3f80_0000, 0, 0, 0x3f80_0000]);
    assert_eq!((words[10], words[11]), (1, 1));
}

#[test]
fn presentation_preserves_copy_vsync_and_signed_destination_coordinates() {
    use crate::graphics::d3d8::commands::{Present, SignedRect};
    let mut bytes = Vec::new();
    append_payload(
        &Command::Present(Present {
            swap_chain: 9,
            swap_effect: 4,
            window: 17,
            source: source(),
            source_rect: None,
            destination_rect: Some(SignedRect {
                left: -10,
                top: -20,
                right: 100,
                bottom: 200,
            }),
            dirty_rects: &[],
            dirty_region: false,
            interval: 0,
        }),
        &mut bytes,
    )
    .unwrap();
    assert_eq!(bytes.len(), 84);
    assert_eq!(
        &bytes[..24],
        &[
            9, 0, 0, 0, 0, 0, 0, 0, 17, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0
        ]
    );
    assert_eq!(
        &bytes[64..80],
        &[
            246, 255, 255, 255, 236, 255, 255, 255, 100, 0, 0, 0, 200, 0, 0, 0
        ]
    );
}

#[test]
fn readback_request_carries_destination_layout_independent_of_front_source_extent() {
    use crate::graphics::d3d8::{
        commands::{ReadbackId, ReadbackKind, ReadbackLayout, ReadbackRequest},
        resources::Format,
    };
    let command = Command::Readback(ReadbackRequest {
        id: ReadbackId {
            epoch: 77,
            sequence: 9,
        },
        source: source(),
        kind: ReadbackKind::FrontBuffer,
        expected: ReadbackLayout {
            format: Format::A8R8G8B8,
            width: 1920,
            height: 1080,
            depth: 1,
            row_pitch: 7680,
            slice_pitch: 8294400,
            length: 8294400,
        },
    });
    assert_eq!(payload_size(&command), Ok((14, 84)));
    let mut bytes = Vec::new();
    append_payload(&command, &mut bytes).unwrap();
    let words: Vec<_> = bytes[48..]
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect();
    assert_eq!(words, [2, 1, 21, 1920, 1080, 1, 7680, 8294400, 8294400]);
}

#[test]
fn empty_present_dirty_region_is_distinct_from_absent_region() {
    use crate::graphics::d3d8::commands::Present;
    let mut absent = Vec::new();
    let mut explicit_empty = Vec::new();
    for (present, bytes) in [(false, &mut absent), (true, &mut explicit_empty)] {
        append_payload(
            &Command::Present(Present {
                swap_chain: 1,
                swap_effect: 3,
                window: 1,
                source: source(),
                source_rect: None,
                destination_rect: None,
                dirty_rects: &[],
                dirty_region: present,
                interval: 0,
            }),
            bytes,
        )
        .unwrap();
    }
    assert_eq!(&absent[20..24], &[0, 0, 0, 0]);
    assert_eq!(&explicit_empty[20..24], &[1, 0, 0, 0]);
    assert_eq!(&absent[24..], &explicit_empty[24..]);
}
