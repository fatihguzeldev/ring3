import assert from 'node:assert/strict';

const fields=[2,3,4,5,6,7,8,9,10,11,12,13,20,21,22,23,24,25,
  76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,91,92,93,94,95,96,99,100,101,102,103];
const names=['link.mode','link.valid','link.context',...fields.map(index=>`link.local${index}`)];

export function coreImports(module) {
  const imports=WebAssembly.Module.imports(module),links=imports.filter(item=>item.name.startsWith('link.'));
  if(links.length)assert.deepEqual(links,names.map(name=>({module:'ring3',name,kind:'global'})),
    'the complete typed compiled-link state is the only added import surface');
  return imports.filter(item=>!item.name.startsWith('link.'));
}

export function linkGlobals(module,shared={}) {
  coreImports(module);
  const globals={};
  for(const item of WebAssembly.Module.imports(module).filter(item=>item.name.startsWith('link.'))) {
    const local=Number(item.name.slice('link.local'.length));
    const type=item.name==='link.local99'?'i64':local>=76&&local<=83||local>=101&&local<=103?'f64':'i32';
    globals[item.name]=shared[item.name]??new WebAssembly.Global({value:type,mutable:true},type==='i64'?0n:0);
    assert(globals[item.name] instanceof WebAssembly.Global,'compiled-link imports are actual Wasm globals');
  }
  return globals;
}

export function instantiateCore(module,imports) {
  return new WebAssembly.Instance(module,{...imports,
    ring3:{...linkGlobals(module,imports?.ring3),...imports?.ring3}});
}
