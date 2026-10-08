import sys
from elftools.elf.elffile import ELFFile
from capstone import *
path=sys.argv[1]; want=set(sys.argv[2:])
f=open(path,"rb"); e=ELFFile(f); data=open(path,"rb").read()
got={}
for secname in (".rela.plt",".rela.dyn"):
    s=e.get_section_by_name(secname)
    if not s: continue
    syms=e.get_section(s["sh_link"])
    for r in s.iter_relocations():
        si=r["r_info_sym"]
        if si: got[r["r_offset"]]=syms.get_symbol(si).name
plt=e.get_section_by_name(".plt"); stubs={}
md=Cs(CS_ARCH_ARM64,CS_MODE_ARM)
ins=list(md.disasm(plt.data(),plt["sh_addr"]))
for i in range(len(ins)-1):
    a,b=ins[i],ins[i+1]
    if a.mnemonic=="adrp" and a.op_str.startswith("x16") and b.mnemonic=="ldr" and "x17" in b.op_str:
        page=int(a.op_str.split("#")[1],16); off=int(b.op_str.split("#")[1].rstrip("]"),16) if "#" in b.op_str else 0
        stubs[a.address]=got.get(page+off,"?")
tg={a:n for a,n in stubs.items() if n in want}
print("stubs:",{hex(a):n for a,n in tg.items()})
text=[s for s in e.iter_segments() if s["p_type"]=="PT_LOAD" and s["p_flags"]&1][0]
w=memoryview(data[text["p_offset"]:text["p_offset"]+text["p_filesz"]//4*4]).cast("I"); base=text["p_vaddr"]
for i,x in enumerate(w):
    if (x&0xfc000000)==0x94000000:  # bl
        imm=x&0x3ffffff
        if imm&(1<<25): imm-=1<<26
        t=base+i*4+imm*4
        if t in tg: print(hex(base+i*4),"->",tg[t])
