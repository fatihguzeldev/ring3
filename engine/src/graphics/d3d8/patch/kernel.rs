// params: surface[128], input plan[128], then 16 (raw byte base, stride) pairs.
// capture precedes later source uploads; evaluate precedes the current vertex shader.
pub const VERSION: u32 = 1;
pub const SOURCE: &str = include_str!("kernel.wgsl");

#[cfg(test)]
mod tests {
    #[test]
    fn engine_owned_capture_and_surface_evaluation_are_valid_wgsl() {
        let module = naga::front::wgsl::parse_str(super::SOURCE)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(super::SOURCE)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
        assert_eq!(
            module
                .entry_points
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["capture", "evaluate"]
        );
    }
}
