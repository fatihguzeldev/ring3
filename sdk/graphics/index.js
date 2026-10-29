import {Reader,PacketStream,bindings,pipeline,shader,require} from './packet.js';
import {Resources} from './resources.js';
import {buildShader,stateBytes,floatBits} from './shaders.js';
import {depthFormats} from './formats.js';
import {ClosedEpochs} from './device/epochs.js';

const probeOrigin=BigInt(Math.floor(performance.timeOrigin*1e6));
const probeStamp=()=>({origin_ns:String(probeOrigin),timestamp_ns:String(probeOrigin+BigInt(Math.floor(performance.now()*1e6)))});
const align=(n,a)=>Math.ceil(n/a)*a;
const color=value=>({r:(value>>>16&255)/255,g:(value>>>8&255)/255,b:(value&255)/255,a:(value>>>24)/255});
const compare=[null,'never','less','equal','less-equal','greater','not-equal','greater-equal','always'];
const blendFactors={1:'zero',2:'one',3:'src',4:'one-minus-src',5:'src-alpha',6:'one-minus-src-alpha',7:'dst-alpha',8:'one-minus-dst-alpha',9:'dst',10:'one-minus-dst',11:'src-alpha-saturated',14:'constant',15:'one-minus-constant'};
const stencil=[null,'keep','zero','replace','increment-clamp','decrement-clamp','invert','increment-wrap','decrement-wrap'];
const topology={1:'point-list',2:'line-list',3:'line-strip',4:'triangle-list',5:'triangle-strip',6:'triangle-list'};
function indexWords(source,at,elements,step) {
  const view=new DataView(source.buffer,source.byteOffset+at,elements*step),indices=new Uint32Array(elements);
  for(let i=0;i<elements;i++)indices[i]=step===2?view.getUint16(i*step,true):view.getUint32(i*step,true);
  return indices;
}

async function checkedShader(device,code,label) {
  const module=device.createShaderModule({code,label}),info=await module.getCompilationInfo();
  const errors=info.messages.filter(m=>m.type==='error');
  if(errors.length)throw new Error(`${label}: ${errors.map(e=>`${e.lineNum}:${e.linePos} ${e.message}`).join('\n')}`);
  return module;
}

class DeviceRenderer {
  constructor(host,epoch) {
    this.host=host;this.device=host.device;this.epoch=epoch;this.stream=new PacketStream();this.pendingEncoder=null;this.accepting=false;this.resources=new Resources(this.device,()=>this.flush());
    this.shaders=new Map();this.states=new Map();this.pipelines=new Map();this.domains=new Map();this.patches=new Map();this.temporary=[];
    this.pipelineInputs=new Map();this.pipelineInputMemoEnabled=false;this.programIdentities=new WeakMap();this.nextProgramIdentity=0;
    this.pipelineInputHits=0;this.pipelineInputMisses=0;this.pipelineBuilds=0;
    this.retainedIndicesEnabled=false;this.retainedIndexUses=0;
    this.draws=0;this.presents=0;this.removed=false;this.gamma=new Float32Array(3*256);
    for(let c=0;c<3;c++)for(let i=0;i<256;i++)this.gamma[c*256+i]=i/255;
  }
  scratch(bytes,usage){const b=this.resources.buffer(bytes,usage);this.temporary.push(b);return b;}
  encoder(){return this.pendingEncoder??=this.device.createCommandEncoder();}
  flush(){const encoder=this.pendingEncoder;if(encoder){this.pendingEncoder=null;this.device.queue.submit([encoder.finish()]);}}
  async accept(packet,frameProbe) {
    require(!this.accepting,'graphics packet already active');this.accepting=true;
    try {
      const decoded=this.stream.decode(packet),readbacks=[],presentCommands=frameProbe?[]:null;require(!this.removed,'command after removal');
      for(const command of decoded.commands){
        require(!this.removed,'command after removal');
        try { const value=await this.execute(command,command.opcode===11&&frameProbe?{...frameProbe,epoch:String(decoded.epoch),sequence:String(decoded.sequence),command_id:String(command.id)}:null);if(value)readbacks.push(value);if(command.opcode===11&&frameProbe){const r=new Reader(command.bytes);presentCommands.push({epoch:String(decoded.epoch),sequence:String(decoded.sequence),command_id:String(command.id),chain:String(r.u64())});} }
        catch(error){error.message=`epoch ${this.epoch}, command ${command.id}, opcode ${command.opcode}: ${error.message}`;throw error;}
      }
      require(!this.removed||!this.stream.pending,'fragment after removal');
      this.flush();
      for(const b of this.temporary)b.destroy();this.temporary.length=0;
      return {epoch:decoded.epoch,sequence:decoded.sequence,readbacks,...(frameProbe?{presentCommands}:{})};
    } finally {
      this.pendingEncoder=null;this.accepting=false;
    }
  }
  async execute(command,frameProbe) {
    const r=new Reader(command.bytes);let result;
    switch(command.opcode){
      case 1:this.resources.create(r);break;
      case 2:this.flush();this.resources.destroy(r.version());break;
      case 3:this.flush();this.resources.upload(r);break;
      case 4:{const handle=r.u32();require(!this.shaders.has(handle),'shader handle reused while live');this.shaders.set(handle,shader(r));break;}
      case 5:require(this.shaders.delete(r.u32()),'stale shader deletion');break;
      case 6:{const id=r.u64();require(!this.states.has(id),'pipeline identity replay');this.states.set(id,pipeline(r));while(this.states.size>256)this.states.delete(this.states.keys().next().value);break;}
      case 7:this.targets=r.targets();break;
      case 8:await this.draw(r);break;
      case 9:await this.clear(r);break;
      case 10:this.copy(r);break;
      case 11:this.flush();await this.present(r,frameProbe);break;
      case 12:{this.generation=r.u64();this.width=r.u32();this.height=r.u32();this.format=r.u32();this.samples=r.u32();this.backBuffers=Array.from({length:r.count(3)},()=>r.surface());this.depth=r.u32()?r.surface():null;break;}
      case 13:{const generation=r.u64(),event=r.u32();require(generation>0n&&event>=1&&event<=3,'device event');if(event===3){this.flush();this.removed=true;}break;}
      case 14:this.flush();result=await this.resources.readback(r);break;
      case 15:this.resources.palettes.set(r.u32(),r.take(1024).slice());break;
      case 16:{r.u32();const bytes=r.take(1536),view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);for(let i=0;i<768;i++)this.gamma[i]=view.getUint16(i*2,true)/65535;break;}
      case 17:{this.cursor={surface:r.surface(),hotspot:r.words(2)};break;}
      case 18:{this.cursorPosition=r.words(3).map(x=>x|0);break;}
      case 19:{const visible=r.u32();require(visible<=1,'cursor visibility');require(!visible,'visible hardware cursor composition is not implemented');this.cursorVisible=false;break;}
      case 20:throw new Error('ProcessVertices needs a GPU-to-D3D vertex declaration writeback and is not implemented');
      case 21:await this.patchKernel(r);break;
      case 22:await this.definePatch(r);break;
      case 23:this.defineDomain(r);break;
      case 24:await this.drawPatch(r);break;
      case 25:{const p=this.patches.get(r.u64());require(p,'stale patch');this.flush();p.controls.destroy();this.patches.delete(p.id);break;}
      case 26:{const id=r.u64(),d=this.domains.get(id);require(d,'stale patch domain');this.flush();d.vertices.destroy();d.indices.destroy();this.domains.delete(id);break;}
      default:throw new Error(`opcode ${command.opcode} not implemented`);
    }
    r.end();return result;
  }
  state(id){const p=this.states.get(id);require(p,`unknown pipeline snapshot ${id}`);return p;}
  program(p){const vertex=this.shaders.get(p.vertexShader),pixel=this.shaders.get(p.pixelShader);require(!p.pixelShader||pixel,'pixel shader unavailable');return {vertex,pixel};}
  programIdentity(program){
    if(!program)return 0;
    let id=this.programIdentities.get(program);
    if(id===undefined){id=++this.nextProgramIdentity;this.programIdentities.set(program,id);}
    return id;
  }
  rememberPipelineInput(input,key){
    if(input===null)return;
    this.pipelineInputs.delete(input);this.pipelineInputs.set(input,key);
    while(this.pipelineInputs.size>256)this.pipelineInputs.delete(this.pipelineInputs.keys().next().value);
  }
  renderTargets(targets){
    const color=this.resources.surface(targets.color),depth=targets.depth?this.resources.surface(targets.depth):null;
    require(!depthFormats.has(color.item.format),'depth format used as color');
    color.item.version=targets.color.version;if(depth)depth.item.version=targets.depth.version;
    return {color,depth};
  }
  textureBindings(p,bindings){
    const textures=bindings.textures.map(t=>t?this.resources.texture(t,p.palette):null);
    const kinds=textures.map(t=>t?(t.item.type===5?'cube':t.item.type===4?'3d':'2d'):null),entries=[];
    for(let i=0;i<8;i++)if(textures[i]){
      const s=p.stages[i],address=value=>({1:'repeat',2:'mirror-repeat',3:'clamp-to-edge',4:'clamp-to-edge',5:'clamp-to-edge'})[value];
      require(address(s[13])&&address(s[14]),'texture address mode');
      const filter=value=>value===1?'nearest':value===2||value===3?'linear':null;
      require(filter(s[16])&&filter(s[17]),'texture filter mode');
      const lodMin=Math.max(0,(s[20]||0)-(bindings.textures[i]?.lod||0));
      const sampler=this.device.createSampler({addressModeU:address(s[13]),addressModeV:address(s[14]),addressModeW:address(s[25]||1),magFilter:filter(s[16]),minFilter:filter(s[17]),mipmapFilter:s[18]===2?'linear':'nearest',lodMinClamp:lodMin,lodMaxClamp:s[18]===0?lodMin:32,maxAnisotropy:s[17]===3&&s[16]===3?Math.max(1,Math.min(16,s[21])):1});
      entries.push({binding:2+i*2,resource:textures[i].view},{binding:3+i*2,resource:sampler});
    }
    return {kinds,entries};
  }
  async gpuPipeline(p,kind,targets,textureKinds,patch=false) {
    const {vertex,pixel}=this.program(p),rs=p.render;
    require(rs[8]===3,'wireframe/point fill mode not implemented');
    require(rs[172]===3,'non-cubic patch interpolation unsupported');
    require(!rs[136]||rs[136]===1,'clipping state');
    require(!rs[152],'user clip planes require clip-distance/discard emulation');
    const colorFormat=targets.color.item.gpuFormat,depthFormat=targets.depth?.item.gpuFormat;
    const input=this.pipelineInputMemoEnabled?JSON.stringify([
      this.programIdentity(vertex),vertex?.declaration?null:p.vertexShader,this.programIdentity(pixel),
      p.render,p.stages,p.lights.filter(light=>light.enabled),p.viewport,
      textureKinds,patch,kind,colorFormat,depthFormat,
    ]):null;
    if(input!==null){
      const key=this.pipelineInputs.get(input);
      if(key!==undefined&&this.pipelines.has(key)){
        this.pipelineInputHits++;
        const cached=this.pipelines.get(key);this.pipelines.delete(key);this.pipelines.set(key,cached);
        this.rememberPipelineInput(input,key);return cached;
      }
      this.pipelineInputs.delete(input);
    }
    this.pipelineInputMisses++;this.pipelineBuilds++;
    const code=buildShader(p,vertex,pixel,textureKinds,{patch});
    const key=JSON.stringify([code,kind,colorFormat,depthFormat,rs[7],rs[14],rs[19],rs[20],rs[22],rs[23],rs[27],rs.slice(52,60),rs[168],rs[171],rs[47]]);
    if(this.pipelines.has(key)){const cached=this.pipelines.get(key);this.pipelines.delete(key);this.pipelines.set(key,cached);this.rememberPipelineInput(input,key);return cached;}
    const module=await checkedShader(this.device,code,'D3D8 vertex/pixel IR');
    const target={format:colorFormat,writeMask:rs[168]&15};
    if(rs[27]){
      let src=rs[19],dst=rs[20];if(src===12){src=5;dst=6;}if(src===13){src=6;dst=5;}
      require(blendFactors[src]&&blendFactors[dst],'blend factor');const operation=({1:'add',2:'subtract',3:'reverse-subtract',4:'min',5:'max'})[rs[171]];require(operation,'blend operation');
      const alphaFactor=f=>f==='src'?'src-alpha':f==='one-minus-src'?'one-minus-src-alpha':f==='dst'?'dst-alpha':f==='one-minus-dst'?'one-minus-dst-alpha':f;
      target.blend={color:{srcFactor:blendFactors[src],dstFactor:blendFactors[dst],operation},alpha:{srcFactor:alphaFactor(blendFactors[src]),dstFactor:alphaFactor(blendFactors[dst]),operation}};
    }
    const layoutEntries=[{binding:0,visibility:GPUShaderStage.VERTEX|GPUShaderStage.FRAGMENT,buffer:{type:'read-only-storage'}},{binding:1,visibility:GPUShaderStage.VERTEX,buffer:{type:'read-only-storage'}}];
    for(let i=0;i<8;i++)if(textureKinds[i])layoutEntries.push({binding:2+i*2,visibility:GPUShaderStage.FRAGMENT,texture:{sampleType:'float',viewDimension:textureKinds[i]}},{binding:3+i*2,visibility:GPUShaderStage.FRAGMENT,sampler:{type:'filtering'}});
    if(patch)layoutEntries.push({binding:18,visibility:GPUShaderStage.VERTEX,buffer:{type:'read-only-storage'}});
    const layout=this.device.createPipelineLayout({bindGroupLayouts:[this.device.createBindGroupLayout({entries:layoutEntries})]});
    const descriptor={label:'D3D8 pipeline',layout,vertex:{module,entryPoint:'vs'},fragment:{module,entryPoint:'fs',targets:[target]},primitive:{topology:kind,frontFace:'ccw',cullMode:rs[22]===1?'none':rs[22]===2?'back':'front'}};
    if(kind.endsWith('strip'))descriptor.primitive.stripIndexFormat='uint32';
    if(depthFormat){
      descriptor.depthStencil={format:depthFormat,depthWriteEnabled:Boolean(rs[7]&&rs[14]),depthCompare:rs[7]?compare[rs[23]]:'always',depthBias:rs[47]|0};
      if(rs[52]){require(depthFormat.includes('stencil'),'stencil enabled without stencil surface');const face={compare:compare[rs[56]],failOp:stencil[rs[53]],depthFailOp:stencil[rs[54]],passOp:stencil[rs[55]]};descriptor.depthStencil.stencilFront=face;descriptor.depthStencil.stencilBack=face;descriptor.depthStencil.stencilReadMask=rs[58];descriptor.depthStencil.stencilWriteMask=rs[59];}
    }
    const gpu=await this.device.createRenderPipelineAsync(descriptor);
    this.pipelines.set(key,gpu);while(this.pipelines.size>128)this.pipelines.delete(this.pipelines.keys().next().value);
    this.rememberPipelineInput(input,key);
    return gpu;
  }
  async draw(r) {
    const id=r.u64(),primitive=r.u32(),count=r.u32(),targets=r.targets(),b=bindings(r),mode=r.u32();let data={};
    if(mode===0)data.first=r.u32();
    else if(mode===1){data.min=r.u32();data.vertices=r.u32();data.start=r.u32();}
    else if(mode===2){data.stride=r.u32();data.bytes=r.take(r.count(256*1024*1024));}
    else if(mode===3){data.min=r.u32();data.vertices=r.u32();data.stride=r.u32();data.format=r.u32();const v=r.count(256*1024*1024),i=r.count(256*1024*1024);data.bytes=r.take(v);data.indices=r.take(i);}
    else throw new Error(`D3D8 host: draw mode ${mode}`);
    require(topology[primitive],'primitive topology');
    const elements=primitive===1?count:primitive===2?2*count:primitive===3?count+1:primitive===4?3*count:count+2;
    let indexBytes=null,retainedIndex=null,indexSource,indexAt,indexStep,first=data.first??0,baseVertex=0;
    if(mode===1||mode===3){
      const format=mode===1?b.indices?.format:data.format,source=mode===1?this.resources.get(b.indices).subs[0].bytes:data.indices;
      require(format===101||format===102,'index format');const step=format===101?2:4,at=mode===1?data.start*step:0;
      require(at+elements*step<=source.length,'index range');indexSource=source;indexAt=at;indexStep=step;
      if(this.retainedIndicesEnabled && mode===1 && (primitive===1 || primitive===2 || primitive===4))retainedIndex=this.resources.retainedIndexBuffer(b.indices,format);
      if(!retainedIndex)indexBytes=indexWords(source,at,elements,step);
      baseVertex=mode===1?b.indices.baseVertex|0:-data.min;
    }
    if(primitive===6){const expanded=new Uint32Array(count*3);for(let i=0;i<count;i++)expanded.set(indexBytes?[indexBytes[0],indexBytes[i+1],indexBytes[i+2]]:[first,first+i+1,first+i+2],i*3);indexBytes=expanded;first=0;}
    const p=this.state(id),output=this.renderTargets(targets),textures=this.textureBindings(p,b),gpu=await this.gpuPipeline(p,topology[primitive],output,textures.kinds);
    const packed=this.resources.packStreams(b,data.bytes?{bytes:data.bytes,stride:data.stride}:null),raw=packed.buffer;if(packed.temporary)this.temporary.push(raw);
    if(retainedIndex && this.resources.packedStreams.get(retainedIndex.key)!==retainedIndex){retainedIndex=null;indexBytes=indexWords(indexSource,indexAt,elements,indexStep);}
    const programs=this.program(p),state=this.scratch(stateBytes(p,programs.vertex,programs.pixel),GPUBufferUsage.STORAGE);
    const entries=[{binding:0,resource:{buffer:state}},{binding:1,resource:{buffer:raw}},...textures.entries];
    const encoder=this.encoder(),pass=encoder.beginRenderPass(this.passDescriptor(output));
    pass.setPipeline(gpu);pass.setBindGroup(0,this.device.createBindGroup({layout:gpu.getBindGroupLayout(0),entries}));this.dynamic(pass,p,output);
    if(retainedIndex){pass.setIndexBuffer(retainedIndex.buffer,'uint32');pass.drawIndexed(elements,1,data.start,baseVertex,0);this.retainedIndexUses++;if(retainedIndex.cacheHit)this.resources.retainedIndexHits++;}
    else if(indexBytes){const index=this.scratch(new Uint8Array(indexBytes.buffer),GPUBufferUsage.INDEX);pass.setIndexBuffer(index,'uint32');pass.drawIndexed(indexBytes.length,1,0,baseVertex,0);}
    else pass.draw(elements,1,first,0);
    pass.end();this.draws++;
  }
  passDescriptor(output) {
    const result={colorAttachments:[{view:output.color.view,loadOp:'load',storeOp:'store'}]};
    if(output.depth){result.depthStencilAttachment={view:output.depth.view,depthLoadOp:'load',depthStoreOp:'store'};if(output.depth.item.gpuFormat.includes('stencil'))Object.assign(result.depthStencilAttachment,{stencilLoadOp:'load',stencilStoreOp:'store'});}
    return result;
  }
  dynamic(pass,p,output){const v=p.viewport,info=output.color.sub.info;require(v.x+v.width<=info.width&&v.y+v.height<=info.height,'viewport exceeds target');pass.setViewport(v.x,v.y,v.width,v.height,v.min,v.max);pass.setScissorRect(v.x,v.y,v.width,v.height);pass.setStencilReference(p.render[57]);pass.setBlendConstant(color(p.render[60]));}
  async clear(r) {
    const targets=r.targets(),flags=r.u32(),value=r.u32(),depth=r.f32(),stencilValue=r.u32(),rectangles=Array.from({length:r.count(65536)},()=>r.rect()),output=this.renderTargets(targets);
    require(flags&&!(flags&~7),'clear flags');
    if(rectangles.length){await this.clearRectangles(output,flags,value,depth,stencilValue,rectangles);return;}
    const descriptor={colorAttachments:[{view:output.color.view,loadOp:flags&1?'clear':'load',storeOp:'store',clearValue:color(value)}]};
    if(output.depth){descriptor.depthStencilAttachment={view:output.depth.view,depthLoadOp:flags&2?'clear':'load',depthStoreOp:'store',depthClearValue:depth};if(output.depth.item.gpuFormat.includes('stencil'))Object.assign(descriptor.depthStencilAttachment,{stencilLoadOp:flags&4?'clear':'load',stencilStoreOp:'store',stencilClearValue:stencilValue});}
    else require(!(flags&6),'clear depth without target');
    this.encoder().beginRenderPass(descriptor).end();
  }
  async clearRectangles(output,flags,value,depth,reference,rectangles){
    require(output.depth||!(flags&6),'clear depth without target');
    const c=color(value),hasDepth=Boolean(output.depth),hasStencil=output.depth?.item.gpuFormat.includes('stencil');
    require(hasStencil||!(flags&4),'stencil clear without stencil target');
    const code=`@vertex fn vs(@builtin(vertex_index)i:u32)->@builtin(position)vec4<f32>{let x=f32((i<<1u)&2u);let y=f32(i&2u);return vec4<f32>(x*2.0-1.0,y*2.0-1.0,0.0,1.0);}
    ${hasDepth?'struct Result{@location(0)color:vec4<f32>,@builtin(frag_depth)depth:f32};':''}
    @fragment fn fs()->${hasDepth?'Result':'@location(0)vec4<f32>'}{${hasDepth?`var out:Result;out.color=vec4<f32>(${c.r},${c.g},${c.b},${c.a});out.depth=${depth};return out;`:`return vec4<f32>(${c.r},${c.g},${c.b},${c.a});`}}`;
    const module=await checkedShader(this.device,code,'D3D8 rectangular clear'),desc={layout:'auto',vertex:{module,entryPoint:'vs'},fragment:{module,entryPoint:'fs',targets:[{format:output.color.item.gpuFormat,writeMask:flags&1?15:0}]},primitive:{topology:'triangle-list'}};
    if(hasDepth){desc.depthStencil={format:output.depth.item.gpuFormat,depthCompare:'always',depthWriteEnabled:Boolean(flags&2)};if(hasStencil){const face={compare:'always',failOp:'keep',depthFailOp:'keep',passOp:flags&4?'replace':'keep'};Object.assign(desc.depthStencil,{stencilFront:face,stencilBack:face,stencilWriteMask:flags&4?255:0});}}
    const pipeline=await this.device.createRenderPipelineAsync(desc),encoder=this.encoder(),pass=encoder.beginRenderPass(this.passDescriptor(output));pass.setPipeline(pipeline);pass.setStencilReference(reference);
    const info=output.color.sub.info;
    for(const [left,top,right,bottom] of rectangles){const x=Math.max(0,left),y=Math.max(0,top),w=Math.min(info.width,right)-x,h=Math.min(info.height,bottom)-y;if(w>0&&h>0){pass.setScissorRect(x,y,w,h);pass.draw(3);}}
    pass.end();
  }
  copy(r) {
    const source=r.surface(),dest=r.surface(),rects=Array.from({length:r.count(65536)},()=>({from:r.rect(),to:r.words(2)})),a=this.resources.surface(source),b=this.resources.surface(dest);
    require(a.item.gpuFormat===b.item.gpuFormat,'copy format mismatch');
    if(!rects.length)rects.push({from:[0,0,a.sub.info.width,a.sub.info.height],to:[0,0]});
    const e=this.encoder();for(const q of rects)e.copyTextureToTexture({texture:a.item.texture,mipLevel:a.sub.info.level,origin:[q.from[0],q.from[1],a.sub.info.face]},{texture:b.item.texture,mipLevel:b.sub.info.level,origin:[q.to[0],q.to[1],b.sub.info.face]},[q.from[2]-q.from[0],q.from[3]-q.from[1],1]);b.item.version=dest.version;
  }
  async present(r,frameProbe) {
    const accepted=frameProbe?probeStamp():null;
    const chain=r.u64(),window=r.u32(),interval=r.u32(),effect=r.u32(),dirty=r.u32(),source=r.surface(),src=r.u32()?r.rect():null,dst=r.u32()?r.words(4).map(x=>x|0):null,rects=Array.from({length:r.count(65536)},()=>r.rect());
    require(chain&&window,'present identity');void interval;void effect;void dirty;void rects;
    const surface=this.resources.surface(source),info=surface.sub.info;
    this.host.resize(dst?Math.max(dst[2],info.width):info.width,dst?Math.max(dst[3],info.height):info.height);
    const diagnostic={type:'present',window,epoch:String(this.epoch),sequence:String(this.stream.sequence),
      presents:++this.presents,draws:this.draws,width:this.host.canvas.width,height:this.host.canvas.height,resourceBytes:this.resources.bytes};
    await this.host.blit(surface.view,src??[0,0,info.width,info.height],dst??[0,0,this.host.canvas.width,this.host.canvas.height],info,this.gamma,frameProbe,timings=>{
      this.host.onDiagnostic({...diagnostic,...(frameProbe?{frame_probe:{schema:1,...frameProbe,chain:String(chain),renderer_accepted:accepted,...timings}}:{})});
    });
  }
  async patchKernel(r){require(r.u32()===1,'patch kernel version');const source=new TextDecoder('utf-8',{fatal:true}).decode(r.take(r.count(1024*1024)));this.patchModule=await checkedShader(this.device,source,'engine patch WGSL');this.patchCapture=await this.device.createComputePipelineAsync({layout:'auto',compute:{module:this.patchModule,entryPoint:'capture'}});this.patchEvaluate=await this.device.createComputePipelineAsync({layout:'auto',compute:{module:this.patchModule,entryPoint:'evaluate'}});}
  patchGeometry(r){const words=r.words(256),streams=Array.from({length:16},()=>r.u32()?{...r.version(),stride:r.u32()}:null);return {words,streams};}
  async capturePatch(geometry){
    require(this.patchCapture,'patch WGSL missing');const params=new Uint32Array(288);params.set(geometry.words);
    const packed=this.resources.packStreams({streams:geometry.streams}),raw=packed.buffer;if(packed.temporary)this.temporary.push(raw);
    // packStreams uses a 128-byte header; kernel parameters hold the same raw byte bases.
    let length=128;for(let i=0;i<16;i++)if(geometry.streams[i]){params[256+2*i]=length;params[257+2*i]=geometry.streams[i].stride;length+=align(this.resources.get(geometry.streams[i]).subs[0].bytes.length,4);}
    const p=this.scratch(new Uint8Array(params.buffer),GPUBufferUsage.STORAGE),count=params[6]*params[7];require(count>0&&count*17*16<=128*1024*1024,'patch control budget');
    const controls=this.device.createBuffer({size:count*17*16,usage:GPUBufferUsage.STORAGE});
    const entries=[{binding:0,resource:{buffer:p}},{binding:1,resource:{buffer:raw}},{binding:2,resource:{buffer:controls}}];
    const encoder=this.encoder(),pass=encoder.beginComputePass();pass.setPipeline(this.patchCapture);pass.setBindGroup(0,this.device.createBindGroup({layout:this.patchCapture.getBindGroupLayout(0),entries}));pass.dispatchWorkgroups(Math.ceil(count/64));pass.end();return {controls,words:geometry.words};
  }
  async definePatch(r){const id=r.u64(),vertexShader=r.u32();require(!this.patches.has(id),'patch identity replay');const patch=await this.capturePatch(this.patchGeometry(r));this.patches.set(id,{id,vertexShader,...patch});}
  defineDomain(r){const id=r.u64(),triangular=r.u32(),continuous=r.u32(),segments=r.words(4),count=r.count(1000000),indexCount=r.count(6000000);require(!this.domains.has(id),'domain identity replay');const vertices=this.resources.buffer(r.take(count*8),GPUBufferUsage.STORAGE),indices=this.resources.buffer(r.take(indexCount*4),GPUBufferUsage.INDEX);this.domains.set(id,{vertices,indices,count,indexCount,triangular,continuous,segments});}
  async drawPatch(r){
    const id=r.u64(),domain=r.u64(),targets=r.targets(),b=bindings(r),mode=r.u32(),patch=mode===0?this.patches.get(r.u64()):await this.capturePatch(this.patchGeometry(r));require(patch&&this.domains.has(domain),'patch/domain not found');
    const d=this.domains.get(domain),params=new Uint32Array(288);params.set(patch.words);const stateBuffer=this.scratch(new Uint8Array(params.buffer),GPUBufferUsage.STORAGE);
    const vertices=this.device.createBuffer({size:d.count*17*16,usage:GPUBufferUsage.STORAGE});this.temporary.push(vertices);if(mode!==0)this.temporary.push(patch.controls);
    const e=this.encoder(),compute=e.beginComputePass();compute.setPipeline(this.patchEvaluate);compute.setBindGroup(0,this.device.createBindGroup({layout:this.patchEvaluate.getBindGroupLayout(0),entries:[{binding:0,resource:{buffer:stateBuffer}},{binding:2,resource:{buffer:patch.controls}},{binding:3,resource:{buffer:d.vertices}},{binding:4,resource:{buffer:vertices}}]}));compute.dispatchWorkgroups(Math.ceil(d.count/64));compute.end();
    const p=this.state(id),output=this.renderTargets(targets),textures=this.textureBindings(p,b),gpu=await this.gpuPipeline(p,'triangle-list',output,textures.kinds,true),program=this.program(p);
    const state=this.scratch(stateBytes(p,program.vertex,program.pixel),GPUBufferUsage.STORAGE),raw=this.scratch(new Uint8Array(128),GPUBufferUsage.STORAGE);
    const entries=[{binding:0,resource:{buffer:state}},{binding:1,resource:{buffer:raw}},{binding:18,resource:{buffer:vertices}},...textures.entries];
    const encoder=this.encoder(),pass=encoder.beginRenderPass(this.passDescriptor(output));pass.setPipeline(gpu);pass.setBindGroup(0,this.device.createBindGroup({layout:gpu.getBindGroupLayout(0),entries}));this.dynamic(pass,p,output);pass.setIndexBuffer(d.indices,'uint32');pass.drawIndexed(d.indexCount);pass.end();this.draws++;
  }
  close(){if(this.closed)return;this.closed=true;this.pendingEncoder=null;this.resources.close();for(const p of this.patches.values())p.controls.destroy();for(const d of this.domains.values()){d.vertices.destroy();d.indices.destroy();}for(const b of this.temporary)b.destroy();this.temporary.length=0;this.patches.clear();this.domains.clear();this.states.clear();this.shaders.clear();this.pipelines.clear();this.pipelineInputs.clear();}
}

export async function createGraphicsHost(canvas,{onDiagnostic=()=>{}}={}) {
  require(navigator.gpu,'WebGPU is unavailable; use a browser with WebGPU enabled');
  const adapter=await navigator.gpu.requestAdapter({powerPreference:'high-performance'});require(adapter,'no WebGPU adapter');
  const device=await adapter.requestDevice({requiredLimits:{maxStorageBuffersPerShaderStage:Math.min(8,adapter.limits.maxStorageBuffersPerShaderStage),maxStorageBufferBindingSize:Math.min(128*1024*1024,adapter.limits.maxStorageBufferBindingSize)}});
  const context=canvas.getContext('webgpu');require(context,'WebGPU canvas unavailable');const format=navigator.gpu.getPreferredCanvasFormat();
  const closedEpochs=new ClosedEpochs(),pendingPresents=new Set();let failed=null,closed=false,submissionAck=false,consuming=0;
  const fail=error=>{if(closed||failed)return;failed=error;onDiagnostic({type:'error',message:error.message});};
  device.lost.then(info=>fail(new Error(`WebGPU device lost: ${info.message}`)));
  device.addEventListener('uncapturederror',event=>fail(event.error));
  const host={device,canvas,context,format,onDiagnostic,sessions:new Map(),
    get submissionAck(){return submissionAck;},
    configureSubmissionAck(enabled){
      require(typeof enabled==='boolean'&&!closed&&!failed,'submission ACK mode');
      require(!consuming&&!pendingPresents.size,'submission ACK changes require an idle drained host');
      submissionAck=enabled;return enabled;
    },
    submissionStatus(){return {enabled:submissionAck,pendingPresentFences:pendingPresents.size,maximumPendingPresents:2,consuming,failed:!!failed};},
    resize(width,height){require(width>0&&height>0&&width<=device.limits.maxTextureDimension2D&&height<=device.limits.maxTextureDimension2D,'canvas extent');if(canvas.width!==width||canvas.height!==height){canvas.width=width;canvas.height=height;}context.configure({device,format,alphaMode:'opaque'});},async blit(view,src,dst,info,gamma,frameProbe,completed){
    if(!host.blitPipeline){
      const module=await checkedShader(device,`@group(0) @binding(0) var image:texture_2d<f32>;@group(0) @binding(1) var smp:sampler;@group(0) @binding(2) var<storage,read> values:array<f32>;
      struct V{@builtin(position)p:vec4<f32>,@location(0)uv:vec2<f32>};@vertex fn vs(@builtin(vertex_index)i:u32)->V{var v:V;let x=f32((i<<1u)&2u);let y=f32(i&2u);v.p=vec4<f32>(x*2.0-1.0,1.0-y*2.0,0.0,1.0);v.uv=vec2<f32>(x,y);return v;}
      @fragment fn fs(v:V)->@location(0)vec4<f32>{let uv=mix(vec2<f32>(values[0],values[1]),vec2<f32>(values[2],values[3]),v.uv);let c=textureSample(image,smp,uv);return vec4<f32>(values[4u+u32(clamp(c.r,0.0,1.0)*255.0)],values[260u+u32(clamp(c.g,0.0,1.0)*255.0)],values[516u+u32(clamp(c.b,0.0,1.0)*255.0)],1.0);}`,'D3D8 presentation');
      host.blitPipeline=await device.createRenderPipelineAsync({layout:'auto',vertex:{module,entryPoint:'vs'},fragment:{module,entryPoint:'fs',targets:[{format}]},primitive:{topology:'triangle-list'}});
    }
    while(pendingPresents.size>=2)await pendingPresents.values().next().value;
    require(!closed,'graphics host closed');require(!failed,failed?.message);
    const values=new Float32Array(772);values.set([src[0]/info.width,src[1]/info.height,src[2]/info.width,src[3]/info.height]);values.set(gamma,4);
    const buffer=device.createBuffer({size:values.byteLength,usage:GPUBufferUsage.STORAGE|GPUBufferUsage.COPY_DST});device.queue.writeBuffer(buffer,0,values);
    const encoder=device.createCommandEncoder(),pass=encoder.beginRenderPass({colorAttachments:[{view:context.getCurrentTexture().createView(),loadOp:'load',storeOp:'store'}]});pass.setPipeline(host.blitPipeline);pass.setBindGroup(0,device.createBindGroup({layout:host.blitPipeline.getBindGroupLayout(0),entries:[{binding:0,resource:view},{binding:1,resource:device.createSampler({minFilter:'linear',magFilter:'linear'})},{binding:2,resource:{buffer}}]}));pass.setViewport(dst[0],dst[1],dst[2]-dst[0],dst[3]-dst[1],0,1);pass.draw(3);pass.end();const commands=encoder.finish(),submit=frameProbe?probeStamp():null;
    let completion;
    try {
      device.queue.submit([commands]);
      completion=device.queue.onSubmittedWorkDone().then(()=>{
        require(!failed,failed?.message);
        if(!closed)completed?.(frameProbe?{gpu_submit:submit,gpu_complete:probeStamp()}:undefined);
      }).finally(()=>buffer.destroy());
    } catch(error){buffer.destroy();throw error;}
    pendingPresents.add(completion);
    void completion.then(()=>pendingPresents.delete(completion),error=>{pendingPresents.delete(completion);fail(error);});
    if(!submissionAck||frameProbe)await completion;
  },async consume(packet,frameProbe){require(!closed,'graphics host closed');require(!failed,failed?.message);const bytes=new Uint8Array(packet);require(bytes.length>=64,'packet header');const epoch=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength).getBigUint64(16,true);let session=host.sessions.get(epoch);if(!session){require(!closedEpochs.has(epoch),'retired device epoch');require(host.sessions.size<64,'device count limit');session=new DeviceRenderer(host,epoch);host.sessions.set(epoch,session);}device.pushErrorScope('validation');let scope=true;consuming++;try{const result=await session.accept(packet,frameProbe),error=await device.popErrorScope();scope=false;if(error)throw error;if(session.removed){session.close();host.sessions.delete(epoch);closedEpochs.add(epoch);}return result;}catch(error){if(scope)await device.popErrorScope().catch(()=>{});fail(error);throw error;}finally{consuming--;}
  },async drain(){require(!closed,'graphics host closed');try{await device.queue.onSubmittedWorkDone();}catch(error){fail(error);}while(pendingPresents.size)await Promise.allSettled([...pendingPresents]);require(!failed,failed?.message);},close(){if(closed)return;closed=true;for(const s of host.sessions.values())s.close();host.sessions.clear();closedEpochs.clear();context.unconfigure();device.destroy();}};
  onDiagnostic({type:'adapter',features:[...adapter.features],limits:{maxTextureDimension2D:device.limits.maxTextureDimension2D,maxStorageBufferBindingSize:device.limits.maxStorageBufferBindingSize}});return host;
}
