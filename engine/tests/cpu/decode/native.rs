#[path = "../../support/cpu/prefix/admission.rs"]
mod support_prefix_admission;

#[path = "instructions/native/data.rs"]
mod decode_data;

#[path = "instructions/native/flow.rs"]
mod decode_flow;

#[path = "instructions/native/integer.rs"]
mod decode_integer;

#[path = "fetch/native/progressive.rs"]
mod decode_progressive_fetch;
