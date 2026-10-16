use super::*;
use crate::graphics::d3d8::{
    commands::{DrawBindings, PatchDraw, PatchSource, PipelineId, StreamResource, Targets},
    patch::{
        Basis, Degree, PatchInfo, Segments, TriPatchInfo,
        domain::{DomainKey, DomainMesh, EdgeStyle},
        program::{InputPlan, SurfaceProgram},
    },
    shader::{Element, ElementType},
};

#[test]
fn patch_program_domain_draw_and_disposal_have_one_compiled_contract() {
    let mut streams = [None; 16];
    streams[2] = Some(StreamResource {
        resource: ResourceVersion {
            id: source().surface.resource,
            version: 8,
        },
        stride: 12,
    });
    let program = SurfaceProgram::compile(PatchInfo::Triangle(TriPatchInfo {
        start: 7,
        vertices: 10,
        basis: Basis::Bezier,
        degree: Degree::Cubic,
    }))
    .unwrap();
    let inputs = InputPlan::compile(
        &[Element {
            stream: 2,
            offset: 0,
            register: 0,
            kind: ElementType::Float3,
        }],
        &[],
    )
    .unwrap();
    let definition = Command::DefinePatch {
        identity: 0x1234,
        vertex_shader: 2,
        program: &program,
        inputs: &inputs,
        streams: &streams,
    };
    assert_eq!(payload_size(&definition), Ok((22, 1128)));
    let mut bytes = Vec::new();
    append_payload(&definition, &mut bytes).unwrap();
    assert_eq!(
        &bytes[..44],
        &[
            0x34, 0x12, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0,
            0, 1, 0, 0, 0, 1, 0, 0, 0, 10, 0, 0, 0, 1, 0, 0, 0
        ]
    );
    assert_eq!(u32::from_le_bytes(bytes[44..48].try_into().unwrap()), 0);
    assert_eq!(u32::from_le_bytes(bytes[52..56].try_into().unwrap()), 3);
    assert_eq!(
        u32::from_le_bytes(bytes[56..60].try_into().unwrap()),
        1.0f32.to_bits()
    );
    let mut mesh = DomainMesh::new().unwrap();
    mesh.rebuild(DomainKey {
        triangular: true,
        segments: Segments::uniform(2.0f32.to_bits(), true).unwrap(),
        style: EdgeStyle::Discrete,
    })
    .unwrap();
    assert_eq!(
        payload_size(&Command::DefinePatchDomain {
            identity: 9,
            mesh: &mesh
        }),
        Ok((23, 168))
    );
    let draw = PatchDraw {
        pipeline: PipelineId(3),
        domain: 9,
        bindings: DrawBindings::default(),
        targets: Targets {
            color: source(),
            depth: None,
        },
        source: PatchSource::Cached { identity: 0x1234 },
    };
    assert_eq!(payload_size(&Command::DrawPatch(&draw)).unwrap().0, 24);
    assert_eq!(
        payload_size(&Command::DeletePatch { identity: 0x1234 }),
        Ok((25, 8))
    );
    assert_eq!(
        payload_size(&Command::DeletePatchDomain { identity: 9 }),
        Ok((26, 8))
    );
    assert_eq!(
        payload_size(&Command::DeletePatch { identity: 0 }),
        Err(CodecError::Identity)
    );
    assert_eq!(
        payload_size(&Command::PatchKernel).unwrap(),
        (21, 8 + crate::graphics::d3d8::patch::kernel::SOURCE.len())
    );
}

#[test]
fn domain_vertices_and_indices_are_literal_semantics_not_raw_tessellator_flags() {
    let mut mesh = DomainMesh::new().unwrap();
    mesh.rebuild(DomainKey {
        triangular: true,
        segments: Segments::uniform(1.0f32.to_bits(), true).unwrap(),
        style: EdgeStyle::Discrete,
    })
    .unwrap();
    let mut bytes = Vec::new();
    append_payload(
        &Command::DefinePatchDomain {
            identity: 1,
            mesh: &mesh,
        },
        &mut bytes,
    )
    .unwrap();
    assert_eq!(bytes.len(), 108);
    assert_eq!(u32::from_le_bytes(bytes[32..36].try_into().unwrap()), 4);
    assert_eq!(u32::from_le_bytes(bytes[36..40].try_into().unwrap()), 9);
    assert_eq!(
        &bytes[48..72],
        &[
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63
        ]
    );
    assert_eq!(&bytes[72..84], &[0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0]);
}
