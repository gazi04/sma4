// Pre-analysis script: builds the GBA memory map around a raw ROM imported at 0x08000000,
// labels I/O registers and marks the ARM entry point.
//@category GBA

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.lang.Register;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.SourceType;
import ghidra.program.model.symbol.SymbolTable;

import java.math.BigInteger;

public class GbaSetup extends GhidraScript {

    private void region(String name, long start, long size, boolean x) throws Exception {
        Memory mem = currentProgram.getMemory();
        Address a = toAddr(start);
        if (mem.getBlock(a) != null) return;
        MemoryBlock b = mem.createUninitializedBlock(name, a, size, false);
        b.setRead(true);
        b.setWrite(true);
        b.setExecute(x);
        if (name.equals("IO")) b.setVolatile(true);
    }

    private void io(SymbolTable st, long addr, String name) throws Exception {
        st.createLabel(toAddr(addr), name, SourceType.IMPORTED);
    }

    @Override
    protected void run() throws Exception {
        MemoryBlock rom = currentProgram.getMemory().getBlock(toAddr(0x08000000L));
        if (rom != null) {
            rom.setName("ROM");
            rom.setWrite(false);
            rom.setExecute(true);
        }
        region("BIOS", 0x00000000L, 0x4000, true);
        region("EWRAM", 0x02000000L, 0x40000, true);
        region("IWRAM", 0x03000000L, 0x8000, true);
        region("IO", 0x04000000L, 0x400, false);
        region("PAL", 0x05000000L, 0x400, false);
        region("VRAM", 0x06000000L, 0x18000, false);
        region("OAM", 0x07000000L, 0x400, false);
        region("SRAM", 0x0E000000L, 0x10000, false);

        SymbolTable st = currentProgram.getSymbolTable();
        long[] addrs = {
            0x000, 0x004, 0x006, 0x008, 0x00A, 0x00C, 0x00E, 0x010, 0x012, 0x014, 0x016,
            0x018, 0x01A, 0x01C, 0x01E, 0x020, 0x028, 0x030, 0x038, 0x040, 0x044, 0x048,
            0x04A, 0x04C, 0x050, 0x052, 0x054, 0x060, 0x062, 0x064, 0x068, 0x06C, 0x070,
            0x072, 0x074, 0x078, 0x07C, 0x080, 0x082, 0x084, 0x088, 0x090, 0x0A0, 0x0A4,
            0x0B0, 0x0B4, 0x0B8, 0x0BA, 0x0BC, 0x0C0, 0x0C4, 0x0C6, 0x0C8, 0x0CC, 0x0D0,
            0x0D2, 0x0D4, 0x0D8, 0x0DC, 0x0DE, 0x100, 0x102, 0x104, 0x106, 0x108, 0x10A,
            0x10C, 0x10E, 0x130, 0x132, 0x200, 0x202, 0x204, 0x208, 0x300,
        };
        String[] names = {
            "REG_DISPCNT", "REG_DISPSTAT", "REG_VCOUNT", "REG_BG0CNT", "REG_BG1CNT",
            "REG_BG2CNT", "REG_BG3CNT", "REG_BG0HOFS", "REG_BG0VOFS", "REG_BG1HOFS",
            "REG_BG1VOFS", "REG_BG2HOFS", "REG_BG2VOFS", "REG_BG3HOFS", "REG_BG3VOFS",
            "REG_BG2PA", "REG_BG2X", "REG_BG3PA", "REG_BG3X", "REG_WIN0H", "REG_WIN0V",
            "REG_WININ", "REG_WINOUT", "REG_MOSAIC", "REG_BLDCNT", "REG_BLDALPHA", "REG_BLDY",
            "REG_SOUND1CNT_L", "REG_SOUND1CNT_H", "REG_SOUND1CNT_X", "REG_SOUND2CNT_L",
            "REG_SOUND2CNT_H", "REG_SOUND3CNT_L", "REG_SOUND3CNT_H", "REG_SOUND3CNT_X",
            "REG_SOUND4CNT_L", "REG_SOUND4CNT_H", "REG_SOUNDCNT_L", "REG_SOUNDCNT_H",
            "REG_SOUNDCNT_X", "REG_SOUNDBIAS", "REG_WAVE_RAM", "REG_FIFO_A", "REG_FIFO_B",
            "REG_DMA0SAD", "REG_DMA0DAD", "REG_DMA0CNT_L", "REG_DMA0CNT_H", "REG_DMA1SAD",
            "REG_DMA1DAD", "REG_DMA1CNT_L", "REG_DMA1CNT_H", "REG_DMA2SAD", "REG_DMA2DAD",
            "REG_DMA2CNT_L", "REG_DMA2CNT_H", "REG_DMA3SAD", "REG_DMA3DAD", "REG_DMA3CNT_L",
            "REG_DMA3CNT_H", "REG_TM0CNT_L", "REG_TM0CNT_H", "REG_TM1CNT_L", "REG_TM1CNT_H",
            "REG_TM2CNT_L", "REG_TM2CNT_H", "REG_TM3CNT_L", "REG_TM3CNT_H", "REG_KEYINPUT",
            "REG_KEYCNT", "REG_IE", "REG_IF", "REG_WAITCNT", "REG_IME", "REG_POSTFLG",
        };
        for (int i = 0; i < addrs.length; i++) io(st, 0x04000000L + addrs[i], names[i]);
        st.createLabel(toAddr(0x03007FFCL), "INTR_VECTOR", SourceType.IMPORTED);
        st.createLabel(toAddr(0x03007FF8L), "INTR_CHECK", SourceType.IMPORTED);

        // ROM header: first word is an ARM branch to the real entry point.
        Address entry = toAddr(0x08000000L);
        Register tmode = currentProgram.getRegister("TMode");
        currentProgram.getProgramContext().setValue(tmode, entry, entry, BigInteger.ZERO);
        st.createLabel(entry, "rom_header_entry", SourceType.IMPORTED);
        st.addExternalEntryPoint(entry);
        disassemble(entry);
        println("GBA memory map set up");
    }
}
