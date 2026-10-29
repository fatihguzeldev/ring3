import {require} from './packet.js';
import {decodeColor,encodeColor,depthFormats} from './formats.js';

const align=(n,a)=>(Math.ceil(n/a)*a);
const packedEntries=1024,packedBytes=32*1024*1024;
export class Resources {
  constructor(device,beforeTextureWrite=()=>{},beforeBufferDestroy=beforeTextureWrite) { this.device=device;this.beforeTextureWrite=beforeTextureWrite;this.beforeBufferDestroy=beforeBufferDestroy;this.items=new Map();this.bytes=0;this.palettes=new Map();this.packedStreams=new Map();this.packedDependencies=new Map();this.packedBytes=0;this.retainedIndexHits=0;this.retainedIndexBuilds=0;this.retainedIndexUpdates=0;this.retainedIndexUploadedBytes=0; }
  create(r) {
    const id=r.resource(),[type,format,usage,pool,width,height,depth,levels,samples,lockable,fvf,reserved]=r.words(12);
    require(!this.items.has(id.key)&&!reserved&&width&&height&&depth&&levels,'resource definition');
    const subs=Array.from({length:r.count(4096)},()=>{const [width,height,depth,rowPitch,slicePitch,length,face,level]=r.words(8);return {width,height,depth,rowPitch,slicePitch,length,face,level};});
    const size=subs.reduce((n,s)=>n+s.length,0);require(size<=256*1024*1024&&this.bytes+size<=512*1024*1024,'host resource byte budget');
    require(samples===0||samples===1||samples===4,`multisample ${samples} cannot be represented by this adapter`);
    const item={...id,type,format,usage,pool,width,height,depth,levels,samples: samples>1?samples:1,lockable,fvf,version:1n,subs:subs.map(info=>({info,bytes:new Uint8Array(info.length),gpuDirty:false})),size};
    if(type===6||type===7){item.buffer=true;}
    else {
      require([1,3,4,5].includes(type),`resource type ${type}`);
      const isDepth=depthFormats.has(format);item.gpuFormat=isDepth?(format===70||format===80?'depth16unorm':format===71?'depth32float':'depth24plus-stencil8'):'rgba8unorm';
      require(!isDepth||type===1,'depth texture is not supported');
      item.texture=this.device.createTexture({label:`D3D8 ${id.key}`,size:[width,height,type===5?6:depth],mipLevelCount:levels,sampleCount:item.samples,dimension:type===4?'3d':'2d',format:item.gpuFormat,usage:GPUTextureUsage.COPY_SRC|GPUTextureUsage.COPY_DST|GPUTextureUsage.TEXTURE_BINDING|((usage&3)||type===1?GPUTextureUsage.RENDER_ATTACHMENT:0)});
      if(item.samples>1)throw new Error('D3D8 host: multisample resource resolve path not implemented');
    }
    this.items.set(id.key,item);this.bytes+=size;return item;
  }
  get(ref) { const i=this.items.get(ref.key);require(i,`stale resource ${ref.key}`);return i; }
  surface(ref) { const item=this.get(ref),sub=item.subs[ref.subresource];require(sub&&!item.buffer,'surface subresource');return {item,sub,view:item.texture.createView({dimension:'2d',baseMipLevel:sub.info.level,mipLevelCount:1,baseArrayLayer:sub.info.face,arrayLayerCount:1})}; }
  destroy(ref) { const i=this.get(ref);this.invalidateStreams(ref.key);i.texture?.destroy();this.items.delete(ref.key);this.bytes-=i.size; }
  upload(r) {
    const ref=r.version(),item=this.get(ref),[index,offset,rowPitch,slicePitch,rowBytes,rows,slices,length]=r.words(8),bytes=r.take(length),sub=item.subs[index];
    require(sub&&ref.version>=item.version&&rowBytes<=rowPitch&&rows&&slices,'upload definition');
    // uploads may share a version or fail after an earlier row has already changed.
    this.invalidateStreams(ref.key,true);
    try {
      for(let z=0;z<slices;z++)for(let y=0;y<rows;y++){
        const at=offset+z*slicePitch+y*rowPitch;require(at+rowBytes<=sub.bytes.length,'upload bounds');sub.bytes.set(bytes.subarray(z*slicePitch+y*rowPitch,z*slicePitch+y*rowPitch+rowBytes),at);
      }
      item.version=ref.version;sub.gpuDirty=false;
      if(!item.buffer)this.flushTexture(item,index);
      else this.updateRetainedIndices(ref,index,offset,rowPitch,slicePitch,rowBytes,rows,slices);
    } catch(error){this.invalidateStreams(ref.key);throw error;}
  }
  invalidateStreams(resource,keepIndices=false) { for(const key of [...(this.packedDependencies.get(resource)??[])])if(!keepIndices||!this.packedStreams.get(key)?.indexFormat)this.releaseStreams(key); }
  updateRetainedIndices(ref,index,offset,rowPitch,slicePitch,rowBytes,rows,slices) {
    for(const key of [...(this.packedDependencies.get(ref.key)??[])]) {
      const cached=this.packedStreams.get(key);if(!cached?.indexFormat)continue;
      if(index!==0){this.releaseStreams(key);continue;}
      if(!rowBytes)continue;
      const source=this.get(ref).subs[0].bytes;this.beforeBufferDestroy();
      for(let z=0;z<slices;z++)for(let y=0;y<rows;y++) {
        const at=offset+z*slicePitch+y*rowPitch,start=at&~3,end=align(at+rowBytes,4),part=source.subarray(start,Math.min(end,source.length));
        const bytes=part.length===end-start?part:new Uint8Array(end-start);if(bytes!==part)bytes.set(part);
        this.device.queue.writeBuffer(cached.buffer,start,bytes);this.retainedIndexUpdates++;this.retainedIndexUploadedBytes+=bytes.length;
      }
    }
  }
  releaseStreams(key) {
    const row=this.packedStreams.get(key);if(!row)return;
    // an encoder can still reference this buffer; submit it before destruction.
    this.beforeBufferDestroy();this.packedStreams.delete(key);this.packedBytes-=row.length;
    for(const resource of row.dependencies){const keys=this.packedDependencies.get(resource);keys.delete(key);if(!keys.size)this.packedDependencies.delete(resource);}
    row.buffer.destroy();
  }
  flushTexture(item,index,paletteIndex=0) {
    if(depthFormats.has(item.format))throw new Error('D3D8 host: CPU depth uploads not implemented');
    const sub=item.subs[index],info=sub.info;
    if((item.format===40||item.format===41)&&!this.palettes.has(paletteIndex)){sub.palettePending=true;return;}
    const rgba=decodeColor(item.format,sub.bytes,info,this.palettes.get(paletteIndex));
    this.beforeTextureWrite();
    this.device.queue.writeTexture({texture:item.texture,mipLevel:info.level,origin:[0,0,info.face] },rgba,{bytesPerRow:info.width*4,rowsPerImage:info.height},[info.width,info.height,info.depth]);
    sub.palettePending=false;
  }
  texture(ref,palette=0) {
    const item=this.get(ref);require(!item.buffer,'buffer used as texture');
    if(item.format===40||item.format===41)for(let i=0;i<item.subs.length;i++)this.flushTexture(item,i,palette);
    return {item,view:item.texture.createView({dimension:item.type===5?'cube':item.type===4?'3d':'2d',baseMipLevel:ref.lod??0})};
  }
  packStreams(bindings,override=null) {
    const chunks=[];let length=128,key='';
    for(let i=0;i<16;i++){
      const b=bindings.streams[i],item=b?this.get(b):null;let bytes=item?item.subs[0].bytes:null,stride=b?.stride??0;
      if(i===0&&override){bytes=override.bytes;stride=override.stride;}
      if(!bytes)continue;
      if(!override)key+=`${i}:${b.key}@${item.version}:${stride}:${bytes.length};`;
      chunks.push({stream:i,at:length,stride,bytes,resource:b?.key});length+=align(bytes.length,4);
    }
    require(length<=this.device.limits.maxStorageBufferBindingSize,'packed vertex storage limit');
    const cached=!override&&this.packedStreams.get(key);
    if(cached){this.packedStreams.delete(key);this.packedStreams.set(key,cached);return cached;}
    const header=new Uint32Array(32);for(const c of chunks){header[c.stream*2]=c.at;header[c.stream*2+1]=c.stride;}
    const raw=new Uint8Array(align(length,4));raw.set(new Uint8Array(header.buffer));for(const c of chunks)raw.set(c.bytes,c.at);
    if(override||length>packedBytes)return {buffer:this.buffer(raw,GPUBufferUsage.STORAGE),temporary:true};
    while(this.packedStreams.size>=packedEntries||this.packedBytes+length>packedBytes)this.releaseStreams(this.packedStreams.keys().next().value);
    const row={buffer:this.buffer(raw,GPUBufferUsage.STORAGE),temporary:false,length,dependencies:[...new Set(chunks.map(c=>c.resource))]};
    this.packedStreams.set(key,row);this.packedBytes+=length;
    for(const resource of row.dependencies){let keys=this.packedDependencies.get(resource);if(!keys){keys=new Set();this.packedDependencies.set(resource,keys);}keys.add(key);}
    return row;
  }
  retainedIndexBuffer(ref,format) {
    const item=this.get(ref),source=item.subs[0].bytes,step=format===101?2:4,length=align(source.length,4);
    if(item.type!==7 || item.format!==format || ![101,102].includes(format) || source.length%step || !length || length>packedBytes)return null;
    const key=`indices:${ref.key}:${format}`,cached=this.packedStreams.get(key);
    if(cached){this.packedStreams.delete(key);this.packedStreams.set(key,cached);cached.cacheHit=true;return cached;}
    while(this.packedStreams.size>=packedEntries || this.packedBytes+length>packedBytes)this.releaseStreams(this.packedStreams.keys().next().value);
    this.beforeBufferDestroy();
    const row={key,buffer:this.buffer(source,GPUBufferUsage.INDEX),indexFormat:format===101?'uint16':'uint32',length,dependencies:[ref.key],cacheHit:false};
    this.packedStreams.set(key,row);this.packedBytes+=length;
    let keys=this.packedDependencies.get(ref.key);if(!keys){keys=new Set();this.packedDependencies.set(ref.key,keys);}keys.add(key);
    this.retainedIndexBuilds++;this.retainedIndexUploadedBytes+=length;return row;
  }
  buffer(bytes,usage) {
    const b=this.device.createBuffer({size:Math.max(4,align(bytes.byteLength,4)),usage:usage|GPUBufferUsage.COPY_DST});
    const padded=bytes.byteLength%4?new Uint8Array(align(bytes.byteLength,4)):bytes;if(padded!==bytes)padded.set(bytes);
    try{this.device.queue.writeBuffer(b,0,padded);}catch(error){b.destroy();throw error;}return b;
  }
  async pixels(ref) {
    const {item,sub}=this.surface(ref);require(!depthFormats.has(item.format),'depth readback is not implemented');
    const i=sub.info,pitch=align(i.width*4,256),b=this.device.createBuffer({size:pitch*i.height*i.depth,usage:GPUBufferUsage.COPY_DST|GPUBufferUsage.MAP_READ});
    const encoder=this.device.createCommandEncoder();encoder.copyTextureToBuffer({texture:item.texture,mipLevel:i.level,origin:[0,0,i.face]},{buffer:b,bytesPerRow:pitch,rowsPerImage:i.height},[i.width,i.height,i.depth]);this.device.queue.submit([encoder.finish()]);
    await b.mapAsync(GPUMapMode.READ);const src=new Uint8Array(b.getMappedRange()),rgba=new Uint8Array(i.width*i.height*i.depth*4);
    for(let z=0;z<i.depth;z++)for(let y=0;y<i.height;y++)rgba.set(src.subarray((z*i.height+y)*pitch,(z*i.height+y)*pitch+i.width*4),(z*i.height+y)*i.width*4);
    b.unmap();b.destroy();return rgba;
  }
  async readback(r) {
    const requestEpoch=r.u64(),requestSequence=r.u64(),source=r.surface(),kind=r.u32();require(r.u32()===1,'readback layout version');
    const [format,width,height,depth,rowPitch,slicePitch,length]=r.words(7),layout={format,width,height,depth,rowPitch,slicePitch,length},item=this.get(source);
    require(length<=256*1024*1024,'readback byte budget');
    let bytes;
    if(item.buffer){require(length===item.subs[0].bytes.length,'buffer readback length');bytes=item.subs[0].bytes.slice();}
    else bytes=encodeColor(format,await this.pixels(source),layout);
    const receipt=new Uint8Array(64),v=new DataView(receipt.buffer);receipt.set(new TextEncoder().encode('R3R8'));v.setUint16(4,2,true);v.setUint16(6,8,true);v.setUint32(8,64+bytes.length,true);v.setUint32(12,bytes.length,true);
    v.setBigUint64(16,requestEpoch,true);v.setBigUint64(24,requestSequence,true);v.setBigUint64(32,source.epoch,true);v.setUint32(40,source.slot,true);v.setUint32(44,source.generation,true);v.setBigUint64(48,source.version,true);v.setUint32(56,source.subresource,true);
    return {receipt,bytes,kind};
  }
  close(){for(const key of this.packedStreams.keys())this.releaseStreams(key);for(const i of this.items.values())i.texture?.destroy();this.items.clear();this.bytes=0;}
}
