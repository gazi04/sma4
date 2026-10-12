// Creates Thumb functions for every entry of the given function-pointer tables.
// Args: pairs of "<table address hex> <entry count>", e.g. 081317d8 6
//@category GBA

import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.app.cmd.function.CreateFunctionCmd;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.lang.Register;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.data.PointerDataType;

import java.math.BigInteger;

public class ThumbPtrTables extends GhidraScript {

    /** Removes contiguous ARM instructions starting at (or covering) a Thumb entry point,
     *  which auto-analysis decoded in the wrong mode. */
    private void clearArmRun(Register tmode, Address fn) throws Exception {
        Instruction insn = getInstructionContaining(fn);
        Address start = insn != null ? insn.getMinAddress() : fn;
        Address end = null;
        while (insn != null) {
            BigInteger t = currentProgram.getProgramContext()
                .getValue(tmode, insn.getMinAddress(), false);
            if (t != null && t.intValue() == 1) break;
            end = insn.getMaxAddress();
            Instruction next = getInstructionAfter(insn);
            if (next == null || !next.getMinAddress().equals(end.next())) break;
            insn = next;
        }
        if (end != null) {
            Function f = getFunctionContaining(start);
            if (f != null && f.getEntryPoint().compareTo(start) >= 0) removeFunction(f);
            clearListing(start, end);
        }
    }

    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        Register tmode = currentProgram.getRegister("TMode");
        int created = 0;
        for (int i = 0; i + 1 < args.length; i += 2) {
            Address table = toAddr(Long.parseLong(args[i], 16));
            int count = Integer.parseInt(args[i + 1]);
            for (int n = 0; n < count; n++) {
                Address slot = table.add(n * 4L);
                long raw = getInt(slot) & 0xFFFFFFFFL;
                clearListing(slot, slot.add(3));
                createData(slot, PointerDataType.dataType);
                if ((raw & 1) == 0) {
                    println("skip non-Thumb entry " + slot + " -> " + Long.toHexString(raw));
                    continue;
                }
                Address fn = toAddr(raw & ~1L);
                if (getFunctionAt(fn) != null) continue;
                try {
                    BigInteger t = currentProgram.getProgramContext().getValue(tmode, fn, false);
                    boolean thumbCode = getInstructionAt(fn) != null && t != null && t.intValue() == 1;
                    if (!thumbCode) {
                        clearArmRun(tmode, fn);
                        currentProgram.getProgramContext().setValue(tmode, fn, fn, BigInteger.ONE);
                        new DisassembleCommand(fn, null, true).applyTo(currentProgram, monitor);
                    }
                    new CreateFunctionCmd(fn).applyTo(currentProgram, monitor);
                    created++;
                } catch (Exception e) {
                    println("failed " + fn + ": " + e.getMessage() + " covering="
                        + currentProgram.getListing().getCodeUnitContaining(fn));
                }
            }
        }
        analyzeChanges(currentProgram);
        println("Created " + created + " Thumb functions");
    }
}
