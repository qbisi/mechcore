// Writes the decompiler's C for named methods, one file per method.
//
// Run headless by scripts/decomp/ghidra.py:
// `-postScript DecompileMethods.java <out-dir> <Namespace.Class$$Method>...`.
// A name that matches no function is reported and skipped; a name shared by
// overloads writes each, numbered.
// @category mechcore

import java.io.File;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileOptions;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Symbol;
import ghidra.program.model.symbol.SymbolType;

public class DecompileMethods extends GhidraScript {

    private static final int TIMEOUT_SECONDS = 120;

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 2) {
            throw new IllegalArgumentException(
                "usage: DecompileMethods.java <out-dir> <Namespace.Class$$Method>...");
        }
        File out = new File(args[0]);
        out.mkdirs();
        DecompInterface decompiler = new DecompInterface();
        DecompileOptions options = new DecompileOptions();
        options.grabFromProgram(currentProgram);
        decompiler.setOptions(options);
        decompiler.toggleCCode(true);
        decompiler.toggleSyntaxTree(false);
        if (!decompiler.openProgram(currentProgram)) {
            throw new IllegalStateException("decompiler: " + decompiler.getLastMessage());
        }
        try {
            for (String name : Arrays.copyOfRange(args, 1, args.length)) {
                List<Function> found = functionsNamed(name);
                if (found.isEmpty()) {
                    println("no function named " + name);
                    continue;
                }
                for (int index = 0; index < found.size(); index++) {
                    Function function = found.get(index);
                    DecompileResults results =
                        decompiler.decompileFunction(function, TIMEOUT_SECONDS, monitor);
                    if (!results.decompileCompleted()) {
                        println(name + ": " + results.getErrorMessage());
                        continue;
                    }
                    String file = name + (found.size() > 1 ? "." + index : "") + ".c";
                    try (PrintWriter writer =
                        new PrintWriter(new File(out, file), StandardCharsets.UTF_8)) {
                        writer.println("// " + name + " @ " + function.getEntryPoint());
                        writer.print(results.getDecompiledFunction().getC());
                    }
                    println("wrote " + file);
                }
            }
        } finally {
            decompiler.dispose();
        }
    }

    /** The functions whose name, or a label at whose entry, is this name. */
    private List<Function> functionsNamed(String name) {
        List<Function> found = new ArrayList<>();
        for (Symbol symbol : currentProgram.getSymbolTable().getSymbols(name)) {
            Function function = symbol.getSymbolType() == SymbolType.FUNCTION
                ? (Function) symbol.getObject()
                : getFunctionAt(symbol.getAddress());
            if (function != null && !found.contains(function)) {
                found.add(function);
            }
        }
        return found;
    }
}
