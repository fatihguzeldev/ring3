import {require} from './packet.js';

export const floatBits = value => new Float32Array(new Uint32Array([value]).buffer)[0];
export const number = value => { require(Number.isFinite(value),'non-finite shader constant');return Number.isInteger(value)?`${value}.0`:`${value}`; };
const vec = a => `vec4<f32>(${a.map(number).join(',')})`;
const rgba = c => [(c>>>16&255)/255,(c>>>8&255)/255,(c&255)/255,(c>>>24)/255];
const lane='xyzw';

export function fvf(value) {
  const elements=[];let offset=0;
  const add=(register,kind)=>{elements.push({stream:0,offset,register,kind});offset+=[4,8,12,16,4,4,4,8][kind];};
  const position=value&14,transformed=position===4,betas=position>=6?(position-4)/2:0,indexed=Boolean(value&0x1000),weights=betas-Number(indexed);
  require(position>=2&&position<=14,'FVF position');add(0,transformed?3:2);
  if(weights){add(1,Math.min(weights,4)-1);if(weights===5)offset+=4;}
  if(indexed)add(2,5);if(value&16)add(3,2);if(value&32)add(4,0);if(value&64)add(5,4);if(value&128)add(6,4);
  for(let i=0;i<((value>>8)&15);i++)add(7+i,[1,2,3,0][value>>>(16+i*2)&3]);
  return {elements,transformed,weights,indexed,stride:offset};
}

const decode = `
@group(0) @binding(0) var<storage,read> state: array<vec4<f32>>;
@group(0) @binding(1) var<storage,read> raw: array<u32>;
fn byte_at(a:u32)->u32{return(raw[a/4u]>>((a%4u)*8u))&255u;}
fn word_at(a:u32)->u32{return byte_at(a)|(byte_at(a+1u)<<8u)|(byte_at(a+2u)<<16u)|(byte_at(a+3u)<<24u);}
fn short_at(a:u32)->f32{return f32(bitcast<i32>((byte_at(a)|(byte_at(a+1u)<<8u))<<16u)>>16u);}
fn decode_attribute(a:u32,kind:u32)->vec4<f32>{
 var v=vec4<f32>(0.0,0.0,0.0,1.0);
 if(kind<=3u){for(var i=0u;i<=kind;i++){v[i]=bitcast<f32>(word_at(a+4u*i));}}
 else if(kind==4u){let w=word_at(a);v=vec4<f32>(f32((w>>16u)&255u),f32((w>>8u)&255u),f32(w&255u),f32(w>>24u))/255.0;}
 else if(kind==5u){v=vec4<f32>(f32(byte_at(a)),f32(byte_at(a+1u)),f32(byte_at(a+2u)),f32(byte_at(a+3u)));}
 else{v.x=short_at(a);v.y=short_at(a+2u);if(kind==7u){v.z=short_at(a+4u);v.w=short_at(a+6u);}}
 return v;
}
fn matrix(i:u32)->mat4x4<f32>{let a=104u+4u*i;return mat4x4<f32>(state[a],state[a+1u],state[a+2u],state[a+3u]);}
struct Output {
 @builtin(position) position:vec4<f32>,
 @location(0) diffuse:vec4<f32>, @location(1) specular:vec4<f32>,
 @location(2) t0:vec4<f32>, @location(3) t1:vec4<f32>, @location(4) t2:vec4<f32>, @location(5) t3:vec4<f32>,
 @location(6) t4:vec4<f32>, @location(7) t5:vec4<f32>, @location(8) t6:vec4<f32>, @location(9) t7:vec4<f32>,
 @location(10) fog:f32,
};`;

function source(s,stage) {
  require(s,'missing shader source');
  const file=['temp','input','constant','address','tex','raster','color','texout'][s.file];require(file,'IR register file');
  let text=s.file===2?`state[${stage===2?'96u+':''}${s.index}u${s.relative?'+u32(clamp(address[0].x,0.0,95.0))':''}]`:`${file}[${s.index}]`;
  text=`(${text}).${[0,1,2,3].map(i=>lane[(s.swizzle>>>(i*2))&3]).join('')}`;
  switch(s.modifier){
    case 0:return text;case 1:return `(-${text})`;case 2:return `(${text}-vec4<f32>(0.5))`;case 3:return `(vec4<f32>(0.5)-${text})`;
    case 4:return `(${text}*2.0-vec4<f32>(1.0))`;case 5:return `(vec4<f32>(1.0)-${text}*2.0)`;case 6:return `(vec4<f32>(1.0)-${text})`;
    case 7:return `(${text}*2.0)`;case 8:return `(${text}*(-2.0))`;case 9:return `(${text}/${text}.z)`;case 10:return `(${text}/${text}.w)`;
    default:throw new Error(`D3D8 host: IR source modifier ${s.modifier}`);
  }
}
function register(d){const n=['temp','input','constant','address','tex','raster','color','texout'][d.file];require(n&&d.file!==2,'IR destination');return `${n}[${d.index}]`;}

function ir(program,textureKinds,p) {
  if(!program)return '';
  let out='',n=0,pad=[];
  for(const ins of program.instructions){
    const d=ins.destination,src=ins.sources.filter(Boolean),s=src.map(x=>source(x,program.stage)),op=ins.operation;
    if(op===0||op===0xfffd)continue;
    if(op===65){out+=`if(any(${register(d)}.xyz<vec3<f32>(0.0))){discard;}\n`;continue;}
    require(!ins.coissue,'coissued shader instruction needs simultaneous register snapshot');
    let value;
    switch(op){
      case 1:value=s[0];break;case 2:value=`${s[0]}+${s[1]}`;break;case 3:value=`${s[0]}-${s[1]}`;break;
      case 4:value=`${s[0]}*${s[1]}+${s[2]}`;break;case 5:value=`${s[0]}*${s[1]}`;break;
      case 6:value=`vec4<f32>(1.0/${s[0]}.x)`;break;case 7:value=`vec4<f32>(inverseSqrt(abs(${s[0]}.x)))`;break;
      case 8:case 9:value=`vec4<f32>(dot(${s[0]}.${op===8?'xyz':'xyzw'},${s[1]}.${op===8?'xyz':'xyzw'}))`;break;
      case 10:value=`min(${s[0]},${s[1]})`;break;case 11:value=`max(${s[0]},${s[1]})`;break;
      case 12:value=`select(vec4<f32>(0.0),vec4<f32>(1.0),${s[0]}<${s[1]})`;break;
      case 13:value=`select(vec4<f32>(0.0),vec4<f32>(1.0),${s[0]}>=${s[1]})`;break;
      case 14:value=`vec4<f32>(exp2(${s[0]}.x))`;break;case 15:value=`vec4<f32>(log2(abs(${s[0]}.x)))`;break;
      case 16:value=`vec4<f32>(1.0,max(${s[0]}.x,0.0),select(0.0,pow(max(${s[0]}.y,0.0),clamp(${s[0]}.w,-128.0,128.0)),${s[0]}.x>0.0),1.0)`;break;
      case 17:value=`vec4<f32>(1.0,${s[0]}.y*${s[1]}.y,${s[0]}.z,${s[1]}.w)`;break;
      case 18:value=`${s[0]}*${s[1]}+(vec4<f32>(1.0)-${s[0]})*${s[2]}`;break;
      case 19:value=`fract(${s[0]})`;break;
      case 20:case 21:case 22:case 23:case 24:{
        const components=({20:4,21:3,22:4,23:3,24:2})[op],width=op<=21?'xyzw':'xyz',terms=[];
        for(let i=0;i<4;i++)terms.push(i<components?`dot(${s[0]}.${width},${source({...src[1],index:src[1].index+i},program.stage)}.${width})`:i===3?'1.0':'0.0');
        value=`vec4<f32>(${terms.join(',')})`;break;
      }
      case 64:value=program.minor===4?s[0]:`input[${7+d.index}]`;break;
      case 66:{const stage=program.minor===4?src[0].index:d.index,coord=program.minor===4?s[0]:`tex[${d.index}]`;value=sample(stage,coord,textureKinds,p);break;}
      case 69:value=sample(d.index,`vec4<f32>(${s[0]}.w,${s[0]}.x,0.0,1.0)`,textureKinds,p);break;
      case 70:value=sample(d.index,`vec4<f32>(${s[0]}.y,${s[0]}.z,0.0,1.0)`,textureKinds,p);break;
      case 71:case 73:pad.push(`dot(tex[${d.index}].xyz,${s[0]}.xyz)`);continue;
      case 72:case 74:{const terms=pad.concat(`dot(tex[${d.index}].xyz,${s[0]}.xyz)`);pad=[];value=sample(d.index,`vec4<f32>(${terms.join(',')},${terms.length===2?'0.0,':''}1.0)`,textureKinds,p);break;}
      case 78:value=`vec4<f32>(exp2(floor(${s[0]}.x)),fract(${s[0]}.x),exp2(${s[0]}.x),1.0)`;break;
      case 79:value=`vec4<f32>(floor(log2(abs(${s[0]}.x))),abs(${s[0]}.x)/exp2(floor(log2(abs(${s[0]}.x)))),log2(abs(${s[0]}.x)),1.0)`;break;
      case 80:value=`select(${s[2]},${s[1]},${s[0]}>vec4<f32>(0.5))`;break;
      case 82:value=sample(d.index,s[0],textureKinds,p);break;
      case 83:value=sample(d.index,`vec4<f32>(dot(tex[${d.index}].xyz,${s[0]}.xyz))`,textureKinds,p);break;
      case 85:value=`vec4<f32>(dot(tex[${d.index}].xyz,${s[0]}.xyz))`;break;
      case 88:value=`select(${s[2]},${s[1]},${s[0]}>=vec4<f32>(0.0))`;break;
      default:throw new Error(`D3D8 host: shader operation ${op} is not implemented`);
    }
    let rhs=`(${value})`;if(d.shift)rhs=`(${rhs}*${number(2**d.shift)})`;if(d.saturate)rhs=`clamp(${rhs},vec4<f32>(0.0),vec4<f32>(1.0))`;
    const tmp=`result${n++}`;out+=`let ${tmp}=${rhs};\n`;
    for(let i=0;i<4;i++)if(d.mask&(1<<i))out+=`${register(d)}.${lane[i]}=${d.file===3?`floor(${tmp}.${lane[i]})`:`${tmp}.${lane[i]}`};\n`;
  }
  return out;
}

function sample(stage,coord,kinds,p){
  require(stage>=0&&stage<8&&kinds[stage],`shader samples unbound stage ${stage}`);
  const dimensions=kinds[stage]==='2d'?2:3,swizzle=dimensions===2?'xy':'xyz',stageState=p.stages[stage];
  const modes=[stageState[13],stageState[14],stageState[25]],lanes='xyz';
  const components=Array.from({length:dimensions},(_,i)=>modes[i]===5?`abs((${coord}).${lanes[i]})`:`(${coord}).${lanes[i]}`);
  const uv=`vec${dimensions}<f32>(${components.join(',')})`;
  const sampled=`textureSampleBias(tex${stage},samp${stage},${uv},${number(floatBits(stageState[19]))})`;
  const border=Array.from({length:dimensions},(_,i)=>i).filter(i=>modes[i]===4).map(i=>`((${coord}).${lanes[i]}>=0.0&&(${coord}).${lanes[i]}<=1.0)`);
  return border.length?`select(${vec(rgba(stageState[15]))},${sampled},${border.join('&&')})`:sampled;
}

const regs=`var temp:array<vec4<f32>,12>;var input:array<vec4<f32>,17>;var address:array<vec4<f32>,1>;var tex:array<vec4<f32>,8>;var raster:array<vec4<f32>,3>;var color:array<vec4<f32>,2>;var texout:array<vec4<f32>,8>;`;

function fixedVertex(p,declaration){
  const rs=p.render,has=reg=>declaration.elements.some(e=>e.register===reg);let out='';
  if(declaration.transformed){
    const v=p.viewport;out+=`let reciprocal=1.0/input[0].w;out.position=vec4<f32>((input[0].x-${number(v.x)}+0.5)/${number(v.width)}*2.0-1.0,1.0-(input[0].y-${number(v.y)}+0.5)/${number(v.height)}*2.0,input[0].z,1.0)*reciprocal;\n`;
  } else {
    out+='var world=matrix(10u);';
    if(rs[151]){
      require(declaration.weights<=4&&[1,2,3,255].includes(rs[151]),`vertex blend ${rs[151]}`);
      const weights=rs[151]===255?0:rs[151];out+='world=mat4x4<f32>();var remaining=1.0;';
      for(let i=0;i<=weights;i++){const w=i<weights?`input[1].${lane[i]}`:'remaining',index=declaration.indexed?`u32(input[2].${lane[i]})`:`${i}u`;out+=`world+=matrix(10u+${index})*${w};`;if(i<weights)out+=`remaining-=input[1].${lane[i]};`;}
    }
    out+='let worldPosition=world*vec4<f32>(input[0].xyz,1.0);let viewPosition=matrix(0u)*worldPosition;out.position=matrix(1u)*viewPosition;\n';
    if(rs[137]){
      const materialSource=(which,defaultIndex)=>rs[141]&&has(rs[which]===1?5:6)&&rs[which]?`input[${rs[which]===1?5:6}]`:`state[${1168+defaultIndex}u]`;
      out+=`let md=${materialSource(145,0)};let ma=${materialSource(147,1)};let ms=${materialSource(146,2)};let me=${materialSource(148,3)};\n`;
      // row-vector normals use the inverse-transpose world transform, computed as cofactors.
      out+='let normalMatrix=matrix(0u)*world;let a=normalMatrix[0].xyz;let b=normalMatrix[1].xyz;let c=normalMatrix[2].xyz;let determinant=dot(a,cross(b,c));var normal=mat3x3<f32>(cross(b,c),cross(c,a),cross(a,b))*input[3].xyz/determinant;';
      if(rs[143])out+='normal=normalize(normal);';
      out+=`var diffuse=(ma*${vec(rgba(rs[139]))}+me).xyz;var spec=vec3<f32>(0.0);\n`;
      for(const light of p.lights.filter(l=>l.enabled)){
        require([1,2,3].includes(light.kind),'light type');const id=light.index;
        out+=`{let lightDirection=(matrix(0u)*${vec([...light.direction,0])}).xyz;var direction=normalize(-lightDirection);var attenuation=1.0;`;
        if(light.kind!==3){out+=`let delta=(matrix(0u)*${vec([...light.position,1])}).xyz-viewPosition.xyz;let distance=length(delta);direction=normalize(delta);attenuation=select(0.0,1.0/max(dot(${vec([...light.attenuation,0])}.xyz,vec3<f32>(1.0,distance,distance*distance)),0.000001),distance<=${number(light.range)});`;
          if(light.kind===2){const inner=Math.cos(light.theta/2),outer=Math.cos(light.phi/2);out+=`let spot=dot(-direction,normalize(lightDirection));attenuation*=pow(clamp((spot-${number(outer)})/max(${number(inner-outer)},0.000001),0.0,1.0),${number(light.falloff)});`;}
        }
        out+=`let ndl=max(dot(normal,direction),0.0);diffuse+=attenuation*(${vec(light.ambient)}.xyz*ma.xyz+${vec(light.diffuse)}.xyz*md.xyz*ndl);`;
        if(rs[29]){out+=`let eye=${rs[142]?'normalize(-viewPosition.xyz)':'vec3<f32>(0.0,0.0,-1.0)'};spec+=attenuation*${vec(light.specular)}.xyz*ms.xyz*select(0.0,pow(max(dot(normal,normalize(direction+eye)),0.0),state[1172u].x),ndl>0.0);`;}
        out+='}\n';void id;
      }
      out+='out.diffuse=vec4<f32>(clamp(diffuse,vec3<f32>(0.0),vec3<f32>(1.0)),md.w);out.specular=vec4<f32>(clamp(spec,vec3<f32>(0.0),vec3<f32>(1.0)),0.0);';
    }
    if(rs[28]&&rs[140])out+=fog('out.fog','abs(viewPosition.z)',rs[140],p);
  }
  if(declaration.transformed||!rs[137])out+=`out.diffuse=${has(5)?'input[5]':'vec4<f32>(1.0)'};out.specular=${has(6)?'input[6]':'vec4<f32>(0.0)'};`;
  for(let i=0;i<8;i++){
    const stage=p.stages[i],index=stage[11]&0xffff,generated=stage[11]>>>16;
    let coord=`input[${7+index}]`;
    if(generated){require(!declaration.transformed,'generated coordinates with XYZRHW');if(generated===1)coord='vec4<f32>(normalize((matrix(0u)*world*vec4<f32>(input[3].xyz,0.0)).xyz),1.0)';else if(generated===2)coord='viewPosition';else if(generated===3)coord='vec4<f32>(reflect(normalize(viewPosition.xyz),normalize((matrix(0u)*world*vec4<f32>(input[3].xyz,0.0)).xyz)),1.0)';else throw new Error(`D3D8 host: texcoord generation ${generated}`);}
    if(stage[24]&255){coord=`matrix(${2+i}u)*${coord}`;if(stage[24]&256){const component=lane[(stage[24]&255)-1];coord=`(${coord})/(${coord}).${component}`;}}
    out+=`out.t${i}=${coord};`;
  }
  return out;
}

function fog(destination,z,mode,p){const rs=p.render;switch(mode){case 1:return `${destination}=clamp(exp(-${number(floatBits(rs[38]))}*${z}),0.0,1.0);`;case 2:return `${destination}=clamp(exp(-pow(${number(floatBits(rs[38]))}*${z},2.0)),0.0,1.0);`;case 3:return `${destination}=clamp((${number(floatBits(rs[37]))}-${z})/(${number(floatBits(rs[37])-floatBits(rs[36]))}),0.0,1.0);`;default:return '';}}

function textureArgument(arg,stage,p){let result;switch(arg&15){case 0:result='input[0]';break;case 1:result='current';break;case 2:result=`sample${stage}`;break;case 3:result=vec(rgba(p.render[60]));break;case 4:result='input[1]';break;case 5:result='temporary';break;default:throw new Error(`D3D8 host: texture argument ${arg}`);}if(arg&32)result=`vec4<f32>(${result}.w)`;if(arg&16)result=`(vec4<f32>(1.0)-${result})`;return result;}
function textureOperation(op,args,stage,p){const [a,b,c]=args;switch(op){case 2:return a;case 3:return b;case 4:return `(${a}*${b})`;case 5:return `(${a}*${b}*2.0)`;case 6:return `(${a}*${b}*4.0)`;case 7:return `(${a}+${b})`;case 8:return `(${a}+${b}-vec4<f32>(0.5))`;case 9:return `((${a}+${b}-vec4<f32>(0.5))*2.0)`;case 10:return `(${a}-${b})`;case 11:return `(${a}+${b}-${a}*${b})`;case 12:return `mix(${b},${a},input[0].w)`;case 13:return `mix(${b},${a},sample${stage}.w)`;case 14:return `mix(${b},${a},${number(rgba(p.render[60])[3])})`;case 15:return `(${a}+${b}*(1.0-sample${stage}.w))`;case 16:return `mix(${b},${a},current.w)`;case 18:return `(${a}+${a}.w*${b})`;case 19:return `vec4<f32>(${a}.xyz*${b}.xyz+vec3<f32>(${a}.w),${a}.w*${b}.w)`;case 20:return `(${a}+(1.0-${a}.w)*${b})`;case 21:return `vec4<f32>((vec3<f32>(1.0)-${a}.xyz)*${b}.xyz+vec3<f32>(${a}.w),${a}.w*${b}.w)`;case 24:return `vec4<f32>(dot(${a}.xyz*2.0-vec3<f32>(1.0),${b}.xyz*2.0-vec3<f32>(1.0)))`;case 25:return `(${a}*${b}+${c})`;case 26:return `(${a}*${b}+(vec4<f32>(1.0)-${a})*${c})`;default:throw new Error(`D3D8 host: texture operation ${op} is not implemented`);}}

export function buildShader(p,vertex,pixel,textureKinds,options={}) {
  const declaration=vertex?.declaration??fvf(p.vertexShader);require(declaration.elements?.length,'empty vertex declaration');
  let vertexBody=`${regs}var out:Output;out.fog=1.0;`;
  for(let i=0;i<17;i++)vertexBody+=`input[${i}]=vec4<f32>(0.0,0.0,0.0,1.0);`;
  for(const e of declaration.elements){require(e.stream<16&&e.register<17&&e.kind<8,'vertex declaration element');vertexBody+=`input[${e.register}]=${options.patch?`patchVertices[vertex*17u+${e.register}u]`:`decode_attribute(raw[${2*e.stream}u]+vertex*raw[${2*e.stream+1}u]+${e.offset}u,${e.kind}u)`};`;}
  if(vertex?.program){vertexBody+=ir(vertex.program,textureKinds,p);vertexBody+='out.position=raster[0];out.diffuse=color[0];out.specular=color[1];out.fog=raster[1].x;';for(let i=0;i<8;i++)vertexBody+=`out.t${i}=texout[${i}];`;}
  else vertexBody+=fixedVertex(p,declaration);
  vertexBody+='return out;';
  let declarations=(p.render[9]===1?decode.replace('@location(0) diffuse','@location(0) @interpolate(flat) diffuse').replace('@location(1) specular','@location(1) @interpolate(flat) specular'):decode)+(options.patch?'\n@group(0) @binding(18) var<storage,read> patchVertices:array<vec4<f32>>;':'');
  for(let i=0;i<8;i++)if(textureKinds[i])declarations+=`\n@group(0) @binding(${2+i*2}) var tex${i}:texture_${textureKinds[i]}<f32>;@group(0) @binding(${3+i*2}) var samp${i}:sampler;`;
  let fragment=`${regs}input[0]=in.diffuse;input[1]=in.specular;`;
  for(let i=0;i<8;i++)fragment+=`input[${7+i}]=in.t${i};tex[${i}]=in.t${i};`;
  if(pixel?.program){fragment+=ir(pixel.program,textureKinds,p);fragment+='var current=temp[0];';}
  else {
    fragment+='var current=in.diffuse;var temporary=vec4<f32>(0.0);';
    for(let i=0;i<8;i++){
      const stage=p.stages[i];if(stage[1]===1)break;
      if(!textureKinds[i]&&(stage[2]&15)===2)break;
      fragment+=`let sample${i}=${textureKinds[i]?sample(i,`in.t${i}`,textureKinds,p):'vec4<f32>(1.0)'};`;
      const args=[2,3,26].map(k=>textureArgument(stage[k],i,p)),alphaArgs=[5,6,27].map(k=>textureArgument(stage[k],i,p));
      const color=textureOperation(stage[1],args,i,p),alpha=stage[4]===1?'current':textureOperation(stage[4],alphaArgs,i,p);
      fragment+=`let stage${i}=clamp(vec4<f32>((${color}).xyz,(${alpha}).w),vec4<f32>(0.0),vec4<f32>(1.0));${stage[28]===5?'temporary':'current'}=stage${i};`;
    }
    if(p.render[29])fragment+='current=vec4<f32>(current.xyz+in.specular.xyz,current.w);';
  }
  const rs=p.render;
  if(rs[15]){const comparisons=['false',null,'<','==','<=','>','!=','>=',null],ref=number((rs[24]&255)/255),op=rs[25];require(op>=1&&op<=8,'alpha compare');fragment+=op===1?'discard;':op===8?'':`if(!(current.w ${comparisons[op]} ${ref})){discard;}`;}
  if(rs[28]){fragment+='var factor=in.fog;';if(rs[35])fragment+=fog('factor','in.position.z',rs[35],p);fragment+=`current=vec4<f32>(mix(${vec(rgba(rs[34]))}.xyz,current.xyz,factor),current.w);`;}
  fragment+='return current;';
  return `${declarations}\n@vertex fn vs(@builtin(vertex_index) vertex:u32)->Output{${vertexBody}}\n@fragment fn fs(in:Output)->@location(0) vec4<f32>{${fragment}}`;
}

export function stateBytes(p,vertex,pixel){
  const out=new Float32Array(1173*4);p.vc.forEach((v,i)=>out.set(v,i*4));p.pc.forEach((v,i)=>out.set(v,(96+i)*4));
  for(const [i,c] of vertex?.declaration?.constants??[])out.set(c,i*4);
  for(const [i,c] of vertex?.program?.constants??[])out.set(c,i*4);
  for(const [i,c] of pixel?.program?.constants??[])out.set(c,(96+i)*4);
  p.transforms.forEach((m,i)=>out.set(m,(104+i*4)*4));out.set(p.material,1168*4);
  return new Uint8Array(out.buffer);
}
