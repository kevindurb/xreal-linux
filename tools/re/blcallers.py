import sys
from elftools.elf.elffile import ELFFile
path=sys.argv[1]; targets={int(x,16) for x in sys.argv[2:]}
e=ELFFile(open(path,"rb")); data=open(path,"rb").read()
text=[s for s in e.iter_segments() if s["p_type"]=="PT_LOAD" and s["p_flags"]&1][0]
w=memoryview(data[text["p_offset"]:text["p_offset"]+text["p_filesz"]//4*4]).cast("I"); base=text["p_vaddr"]
for i,x in enumerate(w):
    if (x&0x7c000000)==0x14000000:
        imm=x&0x3ffffff
        if imm&(1<<25): imm-=1<<26
        t=base+i*4+imm*4
        if t in targets: print(hex(base+i*4),"bl" if x>>31 else "b","->",hex(t))
