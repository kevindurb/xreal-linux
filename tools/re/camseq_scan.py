import sys
from elftools.elf.elffile import ELFFile
path=sys.argv[1]
data=open(path,"rb").read(); e=ELFFile(open(path,"rb"))
text=[s for s in e.iter_segments() if s["p_type"]=="PT_LOAD" and s["p_flags"]&1][0]
base=text["p_vaddr"]; off=text["p_offset"]
w=memoryview(data[off:off+text["p_filesz"]//4*4]).cast("I")
ids={0x273f:"Create",0x2740:"InitSetPixelFormat",0x2741:"InitSetImageResolution",0x2742:"InitSetAutoExposureType",0x2743:"InitSetExposureTime",0x2744:"InitSetGain",0x2745:"Start",0x2746:"Stop",0x2747:"id10055?"}
if len(sys.argv)>2: ids={int(x,16):x for x in sys.argv[2:]}   # or pass your own ids, e.g. 0x2748
hits=[]
for i,x in enumerate(w):
    # MOVZ Wd, #imm16  (sf=0): 0x52800000 | imm16<<5 | Rd ; also MOVZ Xd: 0xD2800000
    if (x&0xff800000) in (0x52800000,0xd2800000):
        imm=(x>>5)&0xffff
        if imm in ids: hits.append((base+i*4,imm,x&0x1f))
for a,imm,rd in hits: print(hex(a), ids[imm], "reg", rd)
print(len(hits),"hits")
