// Post-analysis script: writes one decompiled .c file per function plus an index TSV
// (address, name, size, thumb, callers, callees). Output dir is the first script argument.
//@category GBA

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileOptions;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.lang.Register;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;

import java.io.File;
import java.io.PrintWriter;
import java.math.BigInteger;
import java.util.Set;
import java.util.stream.Collectors;

public class ExportDecomp extends GhidraScript {

    private static String join(Set<Function> fs) {
        return fs.stream()
            .map(f -> f.getEntryPoint().toString())
            .sorted()
            .collect(Collectors.joining(","));
    }

    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        File out = new File(args.length > 0 ? args[0] : "ghidra_export");
        File funcs = new File(out, "functions");
        funcs.mkdirs();

        DecompInterface ifc = new DecompInterface();
        ifc.setOptions(new DecompileOptions());
        ifc.toggleCCode(true);
        ifc.toggleSyntaxTree(false);
        ifc.openProgram(currentProgram);

        Register tmode = currentProgram.getRegister("TMode");
        int n = 0;
        try (PrintWriter idx = new PrintWriter(new File(out, "functions.tsv"))) {
            idx.println("address\tname\tsize\tthumb\tcallers\tcallees");
            FunctionIterator it = currentProgram.getFunctionManager().getFunctions(true);
            while (it.hasNext() && !monitor.isCancelled()) {
                Function f = it.next();
                BigInteger t = currentProgram.getProgramContext()
                    .getValue(tmode, f.getEntryPoint(), false);
                boolean thumb = t != null && t.intValue() == 1;
                idx.printf("%s\t%s\t%d\t%d\t%s\t%s%n",
                    f.getEntryPoint(), f.getName(), f.getBody().getNumAddresses(),
                    thumb ? 1 : 0, join(f.getCallingFunctions(monitor)),
                    join(f.getCalledFunctions(monitor)));

                DecompileResults r = ifc.decompileFunction(f, 60, monitor);
                String c = r.decompileCompleted()
                    ? r.getDecompiledFunction().getC()
                    : "// decompile failed: " + r.getErrorMessage() + "\n";
                try (PrintWriter w = new PrintWriter(
                        new File(funcs, f.getEntryPoint() + "_" + f.getName() + ".c"))) {
                    w.print(c);
                }
                n++;
            }
        }
        ifc.dispose();
        println("Exported " + n + " functions to " + out.getAbsolutePath());
    }
}
