use ring3_engine::{
    cpu::{
        dbt::{CompileLimits, compile_entry_region},
        x86::decode::decode_one,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use ring3_observability::{Config, Field, Level, Observer, categories};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .expect("provide the output .pftrace path");
    let observer = Observer::new(Config {
        level: Level::Detailed,
        ..Config::default()
    })
    .expect("valid capture configuration");
    {
        let _capture = observer.enter();
        let mut memory = AddressSpace::new(1).expect("memory");
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).expect("page range"),
                Permissions::ALL,
            )
            .expect("mapping");
        memory
            .write(GuestAddress(0x1000), &[0xb8, 42, 0, 0, 0, 0xeb, 0xfe])
            .expect("code");
        decode_one(&memory, GuestAddress(0x1000)).expect("decode");
        let region =
            compile_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default())
                .expect("compile");
        observer.instant(
            Level::Basic,
            categories::ENGINE_CPU_CODEGEN,
            "compiled",
            &[Field::unsigned(
                "instructions",
                region.metadata().instructions as u64,
            )],
        );
        observer.counter(
            Level::Basic,
            categories::ENGINE_CPU_CODEGEN,
            "wasm.bytes",
            region.wasm_bytes(&memory).expect("current code").len() as f64,
        );
    }
    std::fs::write(&path, observer.finish())?;
    println!("{path}");
    Ok(())
}
