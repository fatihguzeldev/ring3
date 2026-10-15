use super::*;

fn destination(file: u32, index: u32, mask: u32) -> u32 {
    0x8000_0000 | file << 28 | mask << 16 | index
}
fn source(file: u32, index: u32) -> u32 {
    0x80e4_0000 | file << 28 | index
}

#[test]
fn vertex_shader_preserves_exact_function_and_typed_matrix_semantics() {
    let words = [
        0xfffe_0101,
        0x0002_fffe,
        0x1234,
        0x5678,
        20,
        destination(4, 0, 15),
        source(1, 0),
        source(2, 92),
        0xffff,
    ];
    let program = Program::parse(&words, Stage::Vertex).unwrap();
    assert_eq!(program.tokens(), words);
    let instruction = program.instructions()[0];
    assert_eq!(instruction.operation, Operation::Matrix4x4);
    assert_eq!(
        instruction.sources[1].unwrap().register,
        Register {
            file: RegisterFile::Constant,
            index: 92
        }
    );
    let declaration = Declaration::parse(&[0x2000_0000, 0x4002_0000, u32::MAX]).unwrap();
    assert_eq!(program.validate_declaration(&declaration), Ok(()));
    let missing = Declaration::parse(&[0x2000_0000, 0x4002_0001, u32::MAX]).unwrap();
    assert_eq!(
        program.validate_declaration(&missing),
        Err(ShaderError::InvalidDeclaration)
    );
    let mut invalid = words;
    invalid[7] = source(2, 93);
    assert_eq!(
        Program::parse(&invalid, Stage::Vertex).unwrap_err(),
        ShaderError::InvalidRegister
    );
}

#[test]
fn vertex_register_initialization_and_instruction_budget_are_checked() {
    let words = [0xfffe_0101, 1, destination(4, 0, 15), source(0, 0), 0xffff];
    assert_eq!(
        Program::parse(&words, Stage::Vertex).unwrap_err(),
        ShaderError::UninitializedRegister
    );
    let mut words = vec![0xfffe_0101];
    words.extend(std::iter::repeat_n(0, 128));
    words.extend([1, destination(4, 0, 15), source(1, 0), 0xffff]);
    assert_eq!(
        Program::parse(&words, Stage::Vertex).unwrap_err(),
        ShaderError::InstructionLimit
    );
}

#[test]
fn pixel_versions_and_phase_alpha_lifetime_are_distinguished() {
    for minor in 0..=4 {
        let words = [
            0xffff_0100 | minor,
            1,
            destination(0, 0, 15),
            source(1, 0),
            0xffff,
        ];
        assert_eq!(
            Program::parse(&words, Stage::Pixel)
                .unwrap()
                .version()
                .minor,
            minor as u8
        );
    }
    let words = [
        0xffff_0104,
        66,
        destination(0, 0, 15),
        source(3, 0),
        0xfffd,
        1,
        destination(0, 0, 15),
        source(0, 0),
        0xffff,
    ];
    assert_eq!(
        Program::parse(&words, Stage::Pixel).unwrap_err(),
        ShaderError::UninitializedRegister
    );
    let mut repaired = words;
    repaired[7] = source(1, 0);
    assert!(Program::parse(&repaired, Stage::Pixel).is_ok());
    let invalid = [
        0xffff_0101,
        88,
        destination(0, 0, 15),
        source(1, 0),
        source(1, 0),
        source(1, 0),
        0xffff,
    ];
    assert_eq!(
        Program::parse(&invalid, Stage::Pixel).unwrap_err(),
        ShaderError::InvalidInstruction
    );
}

#[test]
fn coissue_sources_read_the_preceding_pair_input_state() {
    let words = [
        0xffff_0101,
        1,
        destination(0, 0, 7),
        source(1, 0),
        0x4000_0001,
        destination(0, 0, 8),
        source(0, 0),
        0xffff,
    ];
    assert_eq!(
        Program::parse(&words, Stage::Pixel).unwrap_err(),
        ShaderError::UninitializedRegister
    );
    let mut valid = words;
    valid[6] = source(1, 0);
    assert!(Program::parse(&valid, Stage::Pixel).is_ok());
}

#[test]
fn legacy_texture_arithmetic_initializes_only_written_lanes() {
    let partial = [
        0xffff_0101,
        1,
        destination(3, 0, 7),
        source(1, 0),
        1,
        destination(0, 0, 15),
        source(3, 0),
        0xffff,
    ];
    assert_eq!(
        Program::parse(&partial, Stage::Pixel).unwrap_err(),
        ShaderError::UninitializedRegister
    );
    let selected = [
        0xffff_0101,
        1,
        destination(3, 0, 7),
        source(1, 0),
        1,
        destination(0, 0, 7),
        source(3, 0),
        1,
        destination(0, 0, 8),
        source(1, 0),
        0xffff,
    ];
    assert!(Program::parse(&selected, Stage::Pixel).is_ok());
}

#[test]
fn legacy_texture_coissue_reads_the_input_before_both_instructions() {
    let paired = [
        0xffff_0101,
        1,
        destination(0, 0, 7),
        source(1, 0),
        1,
        destination(3, 0, 7),
        source(1, 0),
        0x4000_0001,
        destination(0, 0, 8),
        0xb0aa_0000,
        0xffff,
    ];
    assert_eq!(
        Program::parse(&paired, Stage::Pixel).unwrap_err(),
        ShaderError::UninitializedRegister
    );
    let mut initialized = vec![0xffff_0101, 66, destination(3, 0, 15)];
    initialized.extend_from_slice(&paired[1..]);
    assert!(Program::parse(&initialized, Stage::Pixel).is_ok());
}

#[test]
fn legacy_texture_address_instructions_require_the_full_destination_mask() {
    let masked = [
        0xffff_0101,
        66,
        destination(3, 0, 7),
        1,
        destination(0, 0, 15),
        source(3, 0),
        0xffff,
    ];
    assert_eq!(
        Program::parse(&masked, Stage::Pixel).unwrap_err(),
        ShaderError::InvalidModifier
    );
}

#[test]
fn declaration_stream_offsets_constants_and_getter_tokens_are_exact() {
    let words = [
        0x2000_0000,
        0x4002_0000,
        0x5001_0000,
        0x4004_0005,
        0x2000_0003,
        0x4001_0007,
        0x8200_005f,
        1,
        2,
        3,
        0x8000_0000,
        u32::MAX,
    ];
    let declaration = Declaration::parse(&words).unwrap();
    assert_eq!(declaration.tokens(), words);
    assert_eq!(declaration.strides()[0], 20);
    assert_eq!(declaration.strides()[3], 8);
    assert_eq!(
        declaration.elements()[1],
        Element {
            stream: 0,
            offset: 16,
            register: 5,
            kind: ElementType::Color
        }
    );
    assert_eq!(declaration.constants(), [(95, [1, 2, 3, 0x8000_0000])]);
    assert_eq!(
        Declaration::parse(&[0x4002_0000, u32::MAX]).unwrap_err(),
        ShaderError::InvalidDeclaration
    );
    assert_eq!(
        Declaration::parse(&[0x8200_005f, 1, 2, u32::MAX]).unwrap_err(),
        ShaderError::Truncated
    );
}

#[test]
fn fvf_orders_indexed_blending_colors_and_variable_texture_dimensions() {
    let fvf = Fvf::parse(12 | 0x1000 | 16 | 64 | 0x0200 | (3 << 16) | (2 << 18)).unwrap();
    assert_eq!(fvf.blend_weights, 3);
    assert!(fvf.indexed_blending);
    assert_eq!(fvf.stride, 64);
    assert_eq!(fvf.elements().last().unwrap().kind, ElementType::Float4);
    assert_eq!(Fvf::parse(4 | 16).unwrap_err(), ShaderError::InvalidFvf);
    assert_eq!(Fvf::parse(2 | 0x1000).unwrap_err(), ShaderError::InvalidFvf);
    assert_eq!(Fvf::parse(2 | 0x0900).unwrap_err(), ShaderError::InvalidFvf);
    assert_eq!(Fvf::parse(14).unwrap().stride, 32);
}

#[test]
fn pixel_texture_dependencies_and_projection_channels_are_validated() {
    let words = [
        0xffff_0101,
        66,
        destination(3, 0, 15),
        68,
        destination(3, 1, 15),
        source(3, 0),
        66,
        destination(3, 2, 15),
        1,
        destination(0, 0, 15),
        source(3, 1),
        0xffff,
    ];
    assert!(Program::parse(&words, Stage::Pixel).is_ok());
    let mut consumed = words;
    consumed[10] = source(3, 0);
    assert_eq!(
        Program::parse(&consumed, Stage::Pixel).unwrap_err(),
        ShaderError::InvalidRegister
    );
    let projected = [
        0xffff_0104,
        64,
        destination(0, 1, 3),
        0xbaf4_0000,
        66,
        destination(0, 0, 15),
        source(3, 1),
        0xffff,
    ];
    assert!(Program::parse(&projected, Stage::Pixel).is_ok());
    let uninitialized_alpha = [
        0xffff_0104,
        64,
        destination(0, 0, 7),
        source(3, 0),
        1,
        destination(0, 0, 15),
        source(0, 0),
        0xffff,
    ];
    assert_eq!(
        Program::parse(&uninitialized_alpha, Stage::Pixel).unwrap_err(),
        ShaderError::UninitializedRegister
    );
}
