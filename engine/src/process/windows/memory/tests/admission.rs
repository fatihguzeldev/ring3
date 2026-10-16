use super::*;
use crate::{
    loader::{ModuleCatalog32, ModuleSet32, SystemExport32, SystemExportTarget32, SystemModule32},
    memory::PageRange,
    process::Ole32Config32,
    windows::{WindowsApi32, memory::{MEM_RESERVE, MemoryApi32}},
};
fn com(range: PageRange) -> Ole32Config32 {
    Ole32Config32 {objects:range,max_objects:8,max_threads:8}
}
#[test]
fn configured_unmapped_com_arena_excludes_heap_and_virtual_reservations() {
    let mut engine=engine();
    let arena=PageRange::new(GuestAddress(0x10000),32).unwrap();
    engine.configure_ole32(com(arena)).unwrap();
    assert!(engine.memory().unwrap().permissions_at(GuestAddress(0x10000)).is_err());
    let allocation=call(&mut engine,WindowsApi32::VirtualAlloc.id(),&[0,65536,MEM_RESERVE,4]);
    assert!(allocation>=0x30000);
    let heap=call(&mut engine,MemoryApi32::GetProcessHeap.id(),&[]);
    assert_ne!(heap,0);
    let block=call(&mut engine,MemoryApi32::HeapAlloc.id(),&[heap,0,64]);
    assert_ne!(block,0);
    assert!(block<0x10000||block>=0x30000);
    assert!(engine.memory().unwrap().permissions_at(GuestAddress(0x10000)).is_err());
}
#[test]
fn late_com_configuration_cannot_adopt_an_unmapped_private_reservation() {
    let mut engine=engine();
    assert_eq!(call(&mut engine,WindowsApi32::VirtualAlloc.id(),&[0x200000,65536,MEM_RESERVE,4]),0x200000);
    let version=engine.memory().unwrap().transaction_version();
    assert!(engine.configure_ole32(com(PageRange::new(GuestAddress(0x200000),16).unwrap())).is_err());
    assert!(engine.ole32.is_none());
    assert_eq!(engine.memory().unwrap().transaction_version(),version);
}
#[test]
fn loader_relocates_automatic_system_image_around_virtual_ownership_and_rejects_explicit_overlap() {
    let engine=engine();
    let range=PageRange::new(GuestAddress(0x50000000),16).unwrap();
    let mut catalog=ModuleCatalog32::default();
    catalog.add_system(SystemModule32{name:"reserved.dll".into(),preferred_base:0x50000000,exports:vec![SystemExport32{name:Some("Entry".into()),ordinal:None,target:SystemExportTarget32::Gate(1)}]}).unwrap();
    let set=ModuleSet32::default();
    let plan=catalog.plan_load_with_reservations(&set,engine.memory().unwrap(),"reserved.dll",None,&[range]).unwrap();
    assert_ne!(plan.additions()[0].base(),0x50000000);
    assert!(catalog.plan_load_with_reservations(&set,engine.memory().unwrap(),"reserved.dll",Some(0x50000000),&[range]).is_err());
}
