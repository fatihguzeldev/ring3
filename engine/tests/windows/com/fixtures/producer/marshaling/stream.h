#ifndef RING3_STREAM_COPY_PROOF
#define RING3_STREAM_COPY_PROOF
/* A self-authored application stream. CopyTo must call this guest Write slot,
   including its ordinary failure and deliberately short-success paths. */
typedef struct {const IStreamVtbl *table;ULONG refs,used,position;DWORD mode,owner;BYTE bytes[8];} Sink;
static const GUID stream_iid={12,0,0,{0xc0,0,0,0,0,0,0,0x46}};
static const GUID sequential_iid={0x0c733a30,0x2a1c,0x11ce,{0xad,0xe5,0,0xaa,0,0x44,0x77,0x3d}};
static ULONG WINAPI sink_add(IStream *self) {return ++((Sink *)self)->refs;}
static ULONG WINAPI sink_release(IStream *self) {require(((Sink *)self)->refs!=0,250);return --((Sink *)self)->refs;}
static HRESULT WINAPI sink_query(IStream *self,REFIID iid,void **out) {
    if(!out)return E_POINTER;*out=0;
    if(!same(iid,&unknown_iid)&&!same(iid,&stream_iid)&&!same(iid,&sequential_iid))return E_NOINTERFACE;
    *out=self;sink_add(self);return S_OK;
}
static HRESULT WINAPI sink_read(IStream *self,void *buffer,ULONG length,ULONG *done) {
    Sink *sink=(Sink *)self;if(!buffer)return STG_E_INVALIDPOINTER;
    ULONG count=sink->used-sink->position;if(count>length)count=length;
    for(ULONG i=0;i<count;i++)((BYTE *)buffer)[i]=sink->bytes[sink->position+i];
    sink->position+=count;if(done)*done=count;return count==length?S_OK:S_FALSE;
}
static HRESULT WINAPI sink_write(IStream *self,const void *buffer,ULONG length,ULONG *done) {
    Sink *sink=(Sink *)self;require(sink->refs&&sink->owner==GetCurrentThreadId(),251);
    if(!buffer)return STG_E_INVALIDPOINTER;
    ULONG count=length;if(sink->mode==1&&count>3)count=3;if(sink->mode==2&&count>2)count=2;
    if(count>8-sink->position)return STG_E_MEDIUMFULL;
    for(ULONG i=0;i<count;i++)sink->bytes[sink->position+i]=((const BYTE *)buffer)[i];
    sink->position+=count;if(sink->position>sink->used)sink->used=sink->position;if(done)*done=count;
    record('k',ROLE,(sink->mode<<24)|(length<<8)|count);
    return sink->mode==2?E_FAIL:S_OK;
}
static HRESULT WINAPI sink_seek(IStream *self,LARGE_INTEGER move,DWORD origin,ULARGE_INTEGER *out) {
    Sink *sink=(Sink *)self;if(origin>STREAM_SEEK_END||move.QuadPart < -8||move.QuadPart>8)return STG_E_INVALIDFUNCTION;
    LONGLONG position=(origin==STREAM_SEEK_SET?0:origin==STREAM_SEEK_CUR?sink->position:sink->used)+move.QuadPart;
    if(position<0||position>8)return STG_E_INVALIDFUNCTION;sink->position=(ULONG)position;if(out)out->QuadPart=position;return S_OK;
}
static HRESULT WINAPI sink_resize(IStream *self,ULARGE_INTEGER size) {
    Sink *sink=(Sink *)self;if(size.QuadPart>8)return STG_E_MEDIUMFULL;
    for(ULONG i=sink->used;i<size.LowPart;i++)sink->bytes[i]=0;sink->used=size.LowPart;return S_OK;
}
static HRESULT WINAPI sink_copy(IStream *self,IStream *to,ULARGE_INTEGER size,ULARGE_INTEGER *read,ULARGE_INTEGER *written) {
    (void)self;(void)to;(void)size;(void)read;(void)written;return E_NOTIMPL;
}
static HRESULT WINAPI sink_commit(IStream *self,DWORD flags) {(void)self;return flags?STG_E_INVALIDFLAG:S_OK;}
static HRESULT WINAPI sink_revert(IStream *self) {(void)self;return STG_E_INVALIDFUNCTION;}
static HRESULT WINAPI sink_lock(IStream *self,ULARGE_INTEGER at,ULARGE_INTEGER size,DWORD kind) {(void)self;(void)at;(void)size;(void)kind;return STG_E_INVALIDFUNCTION;}
static HRESULT WINAPI sink_stat(IStream *self,STATSTG *out,DWORD flags) {
    if(!out)return STG_E_INVALIDPOINTER;if(flags>STATFLAG_NONAME)return STG_E_INVALIDFLAG;
    BYTE *bytes=(BYTE *)out;for(unsigned i=0;i<sizeof(*out);i++)bytes[i]=0;out->type=STGTY_STREAM;out->cbSize.QuadPart=((Sink *)self)->used;return S_OK;
}
static HRESULT WINAPI sink_clone(IStream *self,IStream **out) {(void)self;if(!out)return E_POINTER;*out=0;return E_NOTIMPL;}
static const IStreamVtbl sink_table={sink_query,sink_add,sink_release,sink_read,sink_write,sink_seek,sink_resize,sink_copy,sink_commit,sink_revert,sink_lock,sink_lock,sink_stat,sink_clone};
static void prove_stream_copy(IStream *source,const DWORD *packet) {
    for(DWORD mode=0;mode<3;mode++) {
        Sink sink={&sink_table,1,0,0,mode,GetCurrentThreadId(),{0}};
        LARGE_INTEGER zero;zero.QuadPart=0;ULARGE_INTEGER at,read,written,all;all.QuadPart=~(ULONGLONG)0;
        require(IStream_Seek(source,zero,STREAM_SEEK_SET,&at)==S_OK&&at.QuadPart==0,252);
        read.QuadPart=written.QuadPart=~(ULONGLONG)0;
        HRESULT result=IStream_CopyTo(source,(IStream *)&sink,all,&read,&written);
        record('h',ROLE,(DWORD)result);
        HRESULT expected=mode==0?S_OK:mode==1?STG_E_MEDIUMFULL:E_FAIL;
        ULONG count=mode==0?8:mode==1?3:2;
        require(result==expected&&read.QuadPart==8&&written.QuadPart==count,253+mode);
        require(sink.used==count&&sink.refs==1,256);
        for(ULONG i=0;i<count;i++)require(sink.bytes[i]==((const BYTE *)packet)[i],257);
        require(IStream_Seek(source,zero,STREAM_SEEK_CUR,&at)==S_OK&&at.QuadPart==8,258);
        require(IStream_Release((IStream *)&sink)==0,259);
    }
}
#endif
