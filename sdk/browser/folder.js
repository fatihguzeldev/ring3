export function selectedFolder(files) {
  const selected = Array.from(files).sort((a,b) => a.webkitRelativePath.localeCompare(b.webkitRelativePath));
  if (!selected.length) return null;
  if (selected.length > 100000) throw new Error('game folder exceeds 100000 files');
  const root = selected[0].webkitRelativePath.split('/')[0];
  const entries = [], localFiles = new Map(), paths = new Set(), directories = new Set();
  let nextId = 1;
  for (const file of selected) {
    const parts = file.webkitRelativePath.split('/');
    if (parts.shift() !== root || !parts.length || parts.some(part => !part || part === '.' || part === '..' || part.includes('\\'))) throw new Error('invalid selected folder path');
    for (let i = 1; i < parts.length; i++) {
      const path = 'C:\\' + parts.slice(0,i).join('\\'), key = path.toLowerCase();
      if (!directories.has(key)) {
        if (paths.has(key)) throw new Error(`conflicting Windows path: ${path}`);
        directories.add(key); paths.add(key);
        entries.push({id:nextId++,path,size:0,isDirectory:true});
      }
    }
    const path = 'C:\\' + parts.join('\\'), key = path.toLowerCase();
    if (paths.has(key)) throw new Error(`duplicate Windows path: ${path}`);
    paths.add(key);
    const id = nextId++, role = /\.exe$/i.test(file.name) ? 'executable' : /\.dll$/i.test(file.name) ? 'module' : 'file';
    entries.push({id,path,size:file.size,role}); localFiles.set(BigInt(id),file);
  }
  const executables = entries.filter(file => file.role === 'executable').map(file => ({path:file.path,label:file.path}));
  const executable = executables[0]?.path ?? '';
  return {configured:!!executable,root,executables,selectedExecutable:executable,manifest:{root:'C:\\',executable,files:entries},localFiles};
}
