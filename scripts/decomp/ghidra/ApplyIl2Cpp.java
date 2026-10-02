// Names the IL2CPP binary from Il2CppDumper's script.json: a function at
// every method address, named `Namespace.Class$$Method`, and a label at every
// metadata usage (`..._TypeInfo`, `Method$...`).
//
// Without auto-analysis nothing tells the decompiler which calls do not
// return, and a function's body would flow on past its last call into the
// next method. The runtime's throwers are found first: a method's code ends
// with a call to one, so a call target that ends at least NO_RETURN_CALLS
// methods, the instruction just before another method's entry, is marked
// as not returning before any function is made.
//
// Run headless by scripts/decomp/ghidra.py: `-postScript ApplyIl2Cpp.java <script.json>`.
// @category mechcore

import java.io.FileReader;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Map;
import java.util.Set;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.app.cmd.function.CreateFunctionCmd;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.symbol.SourceType;
import ghidra.program.model.symbol.SymbolTable;

public class ApplyIl2Cpp extends GhidraScript {

    private static final int NO_RETURN_CALLS = 100;

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: ApplyIl2Cpp.java <script.json>");
        }
        JsonObject root;
        try (FileReader reader = new FileReader(args[0])) {
            root = JsonParser.parseReader(reader).getAsJsonObject();
        }
        Address base = currentProgram.getImageBase();
        SymbolTable symbols = currentProgram.getSymbolTable();
        FunctionManager functions = currentProgram.getFunctionManager();

        // Every method entry is code: disassemble them all first, so that
        // creating a function does not run into an address another method's
        // flow has not reached.
        JsonArray methods = root.getAsJsonArray("ScriptMethod");
        AddressSet entries = new AddressSet();
        for (JsonElement element : methods) {
            entries.add(base.add(element.getAsJsonObject().get("Address").getAsLong()));
        }
        monitor.setMessage("disassembling " + methods.size() + " methods");
        new DisassembleCommand(entries, null, true).applyTo(currentProgram, monitor);
        markNoReturn(entries, functions);

        int created = 0;
        int labelled = 0;
        Set<Address> seen = new HashSet<>();
        monitor.initialize(methods.size());
        monitor.setMessage("naming methods");
        for (JsonElement element : methods) {
            monitor.checkCancelled();
            monitor.incrementProgress(1);
            JsonObject method = element.getAsJsonObject();
            Address address = base.add(method.get("Address").getAsLong());
            String name = symbolName(method.get("Name").getAsString());
            if (seen.add(address)) {
                Function function = functions.getFunctionAt(address);
                if (function == null) {
                    CreateFunctionCmd command =
                        new CreateFunctionCmd(name, address, null, SourceType.IMPORTED);
                    if (command.applyTo(currentProgram, monitor)) {
                        created++;
                    }
                } else {
                    function.setName(name, SourceType.IMPORTED);
                }
            } else {
                // Identical code folded several methods into one body: the
                // first keeps the function's name, the rest are labels on it.
                symbols.createLabel(address, name, SourceType.IMPORTED);
                labelled++;
            }
        }
        println("functions created: " + created + ", folded methods labelled: " + labelled);

        int metadata = label(root.getAsJsonArray("ScriptMetadata"), "Name", base, symbols);
        int metadataMethods =
            label(root.getAsJsonArray("ScriptMetadataMethod"), "Name", base, symbols);
        println("metadata labels: " + metadata + ", method metadata labels: " + metadataMethods);
    }

    private void markNoReturn(AddressSet entries, FunctionManager functions) throws Exception {
        Map<Address, Integer> endings = new HashMap<>();
        for (Address entry : entries.getAddresses(true)) {
            Instruction before = getInstructionBefore(entry);
            if (before != null && before.getFlowType().isCall()) {
                for (Address target : before.getFlows()) {
                    endings.merge(target, 1, Integer::sum);
                }
            }
        }
        int marked = 0;
        for (Map.Entry<Address, Integer> ending : endings.entrySet()) {
            if (ending.getValue() < NO_RETURN_CALLS) {
                continue;
            }
            Function function = functions.getFunctionAt(ending.getKey());
            if (function == null) {
                disassemble(ending.getKey());
                function = createFunction(ending.getKey(), null);
            }
            if (function != null) {
                function.setNoReturn(true);
                marked++;
                println("no return: " + function.getName() + " ends " + ending.getValue() + " methods");
            }
        }
        println("functions marked as not returning: " + marked);
    }

    private int label(JsonArray entries, String key, Address base, SymbolTable symbols)
            throws Exception {
        int count = 0;
        for (JsonElement element : entries) {
            monitor.checkCancelled();
            JsonObject entry = element.getAsJsonObject();
            Address address = base.add(entry.get("Address").getAsLong());
            symbols.createLabel(address, symbolName(entry.get(key).getAsString()),
                SourceType.IMPORTED);
            count++;
        }
        return count;
    }

    /** A Ghidra symbol name holds no whitespace. */
    private static String symbolName(String name) {
        return name.replaceAll("\\s+", "_");
    }
}
