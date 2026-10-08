import sys
from elftools.elf.elffile import ELFFile
path=sys.argv[1]; lo=int(sys.argv[2],16); hi=int(sys.argv[3],16)
e=ELFFile(open(path,"rb")); data=open(path,"rb").read()
segs=[(s["p_vaddr"],s["p_filesz"],s["p_offset"]) for s in e.iter_segments() if s["p_type"]=="PT_LOAD"]
def rd(va):
    for v,sz,o in segs:
        if v<=va<v+sz:
            off=o+va-v; end=data.find(b"\0",off,off+300)
            s=data[off:end]
            if len(s)>=4 and all(32<=c<127 for c in s): return s.decode()
    return None
text=[s for s in e.iter_segments() if s["p_type"]=="PT_LOAD" and s["p_flags"]&1][0]
base=text["p_vaddr"]; w=memoryview(data[text["p_offset"]:text["p_offset"]+text["p_filesz"]//4*4]).cast("I")
seen=set()
for i in range((lo-base)//4,min((hi-base)//4,len(w)-1)):
    a=w[i]
    if (a&0x9f000000)==0x90000000:
        rd_=a&31; immlo=(a>>29)&3; immhi=(a>>5)&0x7ffff; imm=(immhi<<2)|immlo
        if imm&(1<<20): imm-=1<<21
        page=((base+i*4)&~0xfff)+(imm<<12)
        for j in range(i+1,min(i+4,len(w))):
            b=w[j]
            if (b&0xff800000)==0x91000000 and ((b>>5)&31)==rd_:
                imm12=(b>>10)&0xfff
                if (b>>22)&1: imm12<<=12
                s=rd(page+imm12)
                if s and s not in seen and len(s)>5:
                    seen.add(s); print(hex(base+i*4),s[:140])
