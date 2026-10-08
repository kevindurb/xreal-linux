import sys
from elftools.elf.elffile import ELFFile
from capstone import *
path=sys.argv[1]; start=int(sys.argv[2],16); size=int(sys.argv[3])
f=open(path,"rb"); e=ELFFile(f)
got={}
for secname in (".rela.plt",".rela.dyn"):
    s=e.get_section_by_name(secname)
    if not s: continue
    syms=e.get_section(s["sh_link"])
    for r in s.iter_relocations():
        si=r["r_info_sym"]
        if si: got[r["r_offset"]]=syms.get_symbol(si).name
plt=e.get_section_by_name(".plt"); stubs={}
if plt is not None:
    md0=Cs(CS_ARCH_ARM64,CS_MODE_ARM)
    ins=list(md0.disasm(plt.data(),plt["sh_addr"]))
    for i in range(len(ins)-1):
        a,b=ins[i],ins[i+1]
        if a.mnemonic=="adrp" and a.op_str.startswith("x16") and b.mnemonic=="ldr" and "x17" in b.op_str:
            page=int(a.op_str.split("#")[1],16); off=int(b.op_str.split("#")[1].rstrip("]"),16) if "#" in b.op_str else 0
            stubs[a.address]=got.get(page+off,"?")
for seg in e.iter_segments():
    if seg["p_type"]=="PT_LOAD" and seg["p_vaddr"]<=start<seg["p_vaddr"]+seg["p_filesz"]:
        off=seg["p_offset"]+start-seg["p_vaddr"]; break
f.seek(off); code=f.read(size)
md=Cs(CS_ARCH_ARM64,CS_MODE_ARM)
for i in md.disasm(code,start):
    extra=""
    if i.mnemonic in("bl","b") and i.op_str.startswith("#"):
        t=int(i.op_str[1:],16)
        if t in stubs: extra="   ; "+stubs[t]
    print(f"{i.address:#x}: {i.mnemonic} {i.op_str}{extra}")
