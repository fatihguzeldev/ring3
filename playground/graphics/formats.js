import {require} from './packet.js';

export const DXT1 = 0x31545844, DXT2 = 0x32545844, DXT3 = 0x33545844, DXT4 = 0x34545844, DXT5 = 0x35545844;
export const depthFormats = new Set([70, 71, 73, 75, 77, 79, 80]);
export const bytesPerPixel = new Map([[20,3],[21,4],[22,4],[23,2],[24,2],[25,2],[26,2],[27,1],[28,1],[29,2],[30,2],[31,4],[34,4],[40,2],[41,1],[50,1],[51,2],[52,1],[70,2],[71,4],[73,2],[75,4],[77,4],[79,4],[80,2]]);
const expand = (v, bits) => Math.round(v * 255 / ((1 << bits) - 1));
const rgb565 = word => [expand(word >> 11,5), expand((word >> 5) & 63,6), expand(word & 31,5),255];

export function decodeColor(format, bytes, info, palette) {
  require(info.depth === 1 || info.depth > 1, 'texture depth');
  const out = new Uint8Array(info.width * info.height * info.depth * 4);
  const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if ([DXT1,DXT2,DXT3,DXT4,DXT5].includes(format)) {
    const blockBytes = format === DXT1 ? 8 : 16;
    for (let z = 0; z < info.depth; z++) for (let by = 0; by < Math.ceil(info.height/4); by++) for (let bx = 0; bx < Math.ceil(info.width/4); bx++) {
      const at = z * info.slicePitch + by * info.rowPitch + bx * blockBytes;
      const colorAt = at + (format === DXT1 ? 0 : 8), a = v.getUint16(colorAt,true), b = v.getUint16(colorAt+2,true);
      const c = [rgb565(a),rgb565(b)];
      if (a > b || format !== DXT1) {
        c.push([0,1,2].map(i=>Math.round((2*c[0][i]+c[1][i])/3)).concat(255));
        c.push([0,1,2].map(i=>Math.round((c[0][i]+2*c[1][i])/3)).concat(255));
      } else { c.push([0,1,2].map(i=>Math.round((c[0][i]+c[1][i])/2)).concat(255),[0,0,0,0]); }
      let alpha = null, alphaBits = 0n;
      if (format === DXT4 || format === DXT5) {
        const x=bytes[at],y=bytes[at+1]; alpha=[x,y];
        if (x>y) for(let i=1;i<=6;i++)alpha.push(Math.round(((7-i)*x+i*y)/7));
        else {for(let i=1;i<=4;i++)alpha.push(Math.round(((5-i)*x+i*y)/5));alpha.push(0,255);}
        for(let i=0;i<6;i++)alphaBits|=BigInt(bytes[at+2+i])<<BigInt(8*i);
      }
      const selectors=v.getUint32(colorAt+4,true);
      for(let i=0;i<16;i++){
        const x=bx*4+i%4,y=by*4+(i>>2);if(x>=info.width||y>=info.height)continue;
        const rgba=c[(selectors>>>(i*2))&3].slice();
        if(format===DXT2||format===DXT3)rgba[3]=((bytes[at+(i>>1)]>>((i&1)*4))&15)*17;
        if(alpha)rgba[3]=alpha[Number((alphaBits>>BigInt(i*3))&7n)];
        if((format===DXT2||format===DXT4)&&rgba[3])for(let k=0;k<3;k++)rgba[k]=Math.min(255,Math.round(rgba[k]*255/rgba[3]));
        out.set(rgba,((z*info.height+y)*info.width+x)*4);
      }
    }
    return out;
  }
  const stride=bytesPerPixel.get(format);require(stride && !depthFormats.has(format),`unsupported texture format ${format}`);
  for(let z=0;z<info.depth;z++)for(let y=0;y<info.height;y++)for(let x=0;x<info.width;x++){
    const at=z*info.slicePitch+y*info.rowPitch+x*stride, word=stride>=2?v.getUint16(at,true):bytes[at];
    let c;
    switch(format){
      case 20:c=[bytes[at+2],bytes[at+1],bytes[at],255];break;
      case 21:case 22:c=[bytes[at+2],bytes[at+1],bytes[at],format===21?bytes[at+3]:255];break;
      case 23:c=rgb565(word);break;
      case 24:case 25:c=[expand((word>>10)&31,5),expand((word>>5)&31,5),expand(word&31,5),format===24?255:(word&0x8000?255:0)];break;
      case 26:case 30:c=[((word>>8)&15)*17,((word>>4)&15)*17,(word&15)*17,format===30?255:((word>>12)&15)*17];break;
      case 27:case 29:c=[expand((word>>5)&7,3),expand((word>>2)&7,3),expand(word&3,2),format===29?bytes[at+1]:255];break;
      case 28:c=[255,255,255,word];break;
      case 31:{const w=v.getUint32(at,true);c=[Math.round((w&1023)*255/1023),Math.round(((w>>>10)&1023)*255/1023),Math.round(((w>>>20)&1023)*255/1023),(w>>>30)*85];break;}
      case 34:c=[Math.round(word*255/65535),Math.round(v.getUint16(at+2,true)*255/65535),0,255];break;
      case 40:case 41:require(palette,'palettized texture without palette');c=Array.from(palette.subarray(bytes[at]*4,bytes[at]*4+4));c[3]=format===40?bytes[at+1]:255;break;
      case 50:c=[word,word,word,255];break;
      case 51:c=[bytes[at],bytes[at],bytes[at],bytes[at+1]];break;
      case 52:c=[(word&15)*17,(word&15)*17,(word&15)*17,(word>>4)*17];break;
      default:throw new Error(`D3D8 host: unsupported texture format ${format}`);
    }
    out.set(c,((z*info.height+y)*info.width+x)*4);
  }
  return out;
}

export function encodeColor(format, rgba, layout) {
  const bpp=bytesPerPixel.get(format);require(bpp&&!depthFormats.has(format),`readback format ${format}`);
  const out=new Uint8Array(layout.length),v=new DataView(out.buffer);
  for(let z=0;z<layout.depth;z++)for(let y=0;y<layout.height;y++)for(let x=0;x<layout.width;x++){
    const from=((z*layout.height+y)*layout.width+x)*4,[r,g,b,a]=rgba.subarray(from,from+4),at=z*layout.slicePitch+y*layout.rowPitch+x*bpp;
    const q=(x,n)=>Math.round(x*((1<<n)-1)/255);
    switch(format){
      case 20:out.set([b,g,r],at);break;
      case 21:case 22:out.set([b,g,r,format===22?255:a],at);break;
      case 23:v.setUint16(at,(q(r,5)<<11)|(q(g,6)<<5)|q(b,5),true);break;
      case 24:case 25:v.setUint16(at,((format===24||a>=128?1:0)<<15)|(q(r,5)<<10)|(q(g,5)<<5)|q(b,5),true);break;
      case 26:case 30:v.setUint16(at,((format===30?15:q(a,4))<<12)|(q(r,4)<<8)|(q(g,4)<<4)|q(b,4),true);break;
      case 28:out[at]=a;break;
      case 50:out[at]=r;break;
      case 51:out.set([r,a],at);break;
      default:throw new Error(`D3D8 host: readback encoding ${format} is not implemented`);
    }
  }
  return out;
}
