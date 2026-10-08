import sys
from elftools.elf.elffile import ELFFile
path=sys.argv[1]
data=open(path,"rb").read(); e=ELFFile(open(path,"rb"))
segs=[(s["p_vaddr"],s["p_filesz"],s["p_offset"]) for s in e.iter_segments() if s["p_type"]=="PT_LOAD"]
def va_of(off):
    for v,sz,o in segs:
        if o<=off<o+sz: return v+off-o
text=[s for s in e.iter_segments() if s["p_type"]=="PT_LOAD" and s["p_flags"]&1][0]
base=text["p_vaddr"]; toff=text["p_offset"]; n=text["p_filesz"]//4
w=memoryview(data[toff:toff+n*4]).cast("I")
def prologue_before(addr):
    i=(addr-base)//4
    for k in range(i, max(i-1200,0), -1):
        x=w[k]
        if (x&0xffc003e0)==0xd10003e0 and (x&0x1f)==31:   # sub sp, sp, #imm
            return base+k*4
    return None
names=["start, format=","start, resolution=","start, type=","start, time=","start, gain=","Call NRGrayscaleCameraCreate start","Call NRGrayscaleCameraStart","Call NRGrayscaleCameraStop"]
for key in ["Call NRGrayscaleCameraInitSetPixelFormat start","Call NRGrayscaleCameraInitSetImageResolution start","Call NRGrayscaleCameraInitSetAutoExposureType start","Call NRGrayscaleCameraInitSetExposureTime start","Call NRGrayscaleCameraInitSetGain start","Call NRGrayscaleCameraCreate start","Call NRGrayscaleCameraStart","Call NRGrayscaleCameraStop"]:
    pos=0; found=False
    while True:
        off=data.find(key.encode(),pos)
        if off<0: break
        pos=off+1
        va=va_of(off)
        if va is None: continue
        # the string may be preceded by "[{}] " so look for refs to the string start minus 0..6 bytes
        cand={va-d for d in range(0,8)}
        for i in range(n-1):
            a=w[i]
            if (a&0x9f000000)==0x90000000:
                rd=a&31; immlo=(a>>29)&3; immhi=(a>>5)&0x7ffff; imm=(immhi<<2)|immlo
                if imm&(1<<20): imm-=1<<21
                page=((base+i*4)&~0xfff)+(imm<<12)
                for j in range(i+1,min(i+5,n)):
                    b=w[j]
                    if (b&0xff800000)==0x91000000 and ((b>>5)&31)==rd:
                        im=(b>>10)&0xfff
                        if (b>>22)&1: im<<=12
                        if page+im in cand:
                            ref=base+i*4; print(key[5:70].ljust(52),"ref",hex(ref),"function starts",hex(prologue_before(ref) or 0)); found=True
    if not found: print(key[5:70].ljust(52),"no code reference found")
