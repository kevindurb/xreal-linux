import sys,struct
from elftools.elf.elffile import ELFFile
path=sys.argv[1]; targets=[int(x,16) for x in sys.argv[2:]]
e=ELFFile(open(path,"rb")); data=open(path,"rb").read()
text=[s for s in e.iter_segments() if s["p_type"]=="PT_LOAD" and s["p_flags"]&1][0]
base=text["p_vaddr"]; off=text["p_offset"]; n=text["p_filesz"]//4
w=memoryview(data[off:off+n*4]).cast("I")
tset=set(targets)
for i in range(n-1):
    a=w[i]
    if (a&0x9f000000)==0x90000000:  # adrp
        rd=a&31
        immlo=(a>>29)&3; immhi=(a>>5)&0x7ffff
        imm=((immhi<<2)|immlo)
        if imm&(1<<20): imm-=1<<21
        pc=base+i*4
        page=(pc&~0xfff)+(imm<<12)
        for j in range(i+1,min(i+6,n)):
            b=w[j]
            if (b&0xff800000)==0x91000000 and ((b>>5)&31)==rd:   # add xd, xn, #imm12
                imm12=(b>>10)&0xfff
                if (b>>22)&1: imm12<<=12
                addr=page+imm12
                if addr in tset: print(hex(addr),"xref at",hex(base+i*4),"(add at",hex(base+j*4)+")")
