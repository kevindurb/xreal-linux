import sys
from elftools.elf.elffile import ELFFile
from capstone import *
from capstone.arm64 import *
path="cg31/x/lib/arm64-v8a/libnr_service.so"
data=open(path,"rb").read(); e=ELFFile(open(path,"rb"))
text=[s for s in e.iter_segments() if s["p_type"]=="PT_LOAD" and s["p_flags"]&1][0]
base=text["p_vaddr"]; code=data[text["p_offset"]:text["p_offset"]+text["p_filesz"]]
rel={}
for r in e.get_section_by_name(".rela.dyn").iter_relocations():
    if r["r_info_type"]==1027: rel[r["r_offset"]]=r["r_addend"]
inv={}
for k,v in rel.items(): inv.setdefault(v,[]).append(k)
def vtable_of(method):
    out=[]
    for slot in inv.get(method,[]):
        # walk back to vtable start: vtable P such that slot in [P+16 ...]; P+0 offset_to_top(0, no rel), P+8 typeinfo
        k=slot
        while (k-8) in rel: k-=8
        out.append((slot,k))
    return out
def ids_for_vtable_start(first_virtual_slot):
    P=first_virtual_slot-16
    gots=[g for g,a in rel.items() if a==P]
    return P,gots
m=int(sys.argv[1],16)
for slot,start in vtable_of(m):
    P,gots=ids_for_vtable_start(start)
    print("method",hex(m),"slot",hex(slot),"virt0",hex(start),"P",hex(P),"GOT slots",[hex(g) for g in gots])
    # find code refs to GOT slots then nearest id registration
    for g in gots:
        page=g&~0xfff; off=g&0xfff
        w=memoryview(code[:len(code)//4*4]).cast("I")
        for i in range(len(w)-1):
            x=w[i]
            if (x&0x9f000000)==0x90000000:
                immlo=(x>>29)&3; immhi=(x>>5)&0x7ffff; imm=(immhi<<2)|immlo
                if imm&(1<<20): imm-=1<<21
                pg=((base+i*4)&~0xfff)+(imm<<12)
                if pg==page:
                    rd=x&31
                    for j in range(i+1,min(i+6,len(w))):
                        y=w[j]
                        # ldr xt,[xn,#imm]: 0xF9400000
                        if (y&0xffc00000)==0xf9400000 and ((y>>5)&31)==rd and ((y>>10)&0xfff)*8==off:
                            print("   code ref at",hex(base+i*4))
