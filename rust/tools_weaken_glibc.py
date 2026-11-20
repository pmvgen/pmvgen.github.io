# Marks the GLIBC_2.39 version need of the Linux binary as weak: the only symbols that need it
# (pidfd_spawnp, pidfd_getpid) are weak references Rust's std checks at runtime, so the program
# also starts on older glibc (Linux Mint 21: 2.35) instead of refusing with "GLIBC_2.39 not found".
import sys
from elftools.elf.elffile import ELFFile
path = sys.argv[1]
f = open(path, "rb")
elf = ELFFile(f)
sec = elf.get_section_by_name(".gnu.version_r")
dynstr = elf.get_section_by_name(".dynstr")
off = sec["sh_offset"]
data = sec.data()
patched = []
blob = bytearray(open(path, "rb").read())
pos = 0
while True:
    vn_version, vn_cnt, vn_file, vn_aux, vn_next = __import__("struct").unpack_from("<HHIII", data, pos)
    apos = pos + vn_aux
    for _ in range(vn_cnt):
        vna_hash, vna_flags, vna_other, vna_name, vna_next = __import__("struct").unpack_from("<IHHII", data, apos)
        name = dynstr.get_string(vna_name)
        if name == "GLIBC_2.39":
            blob[off + apos + 4: off + apos + 6] = (vna_flags | 0x2).to_bytes(2, "little")
            patched.append(name)
        if vna_next == 0:
            break
        apos += vna_next
    if vn_next == 0:
        break
    pos += vn_next
f.close()
open(path, "wb").write(bytes(blob))
print("weakened:", patched or "nothing")
