// Types the IL2CPP binary from Il2CppDumper's il2cpp.h and script.json: the
// header's structs into the program, each method's C signature on its
// function, and each metadata usage's type (`Namespace_Class_c*`) on its
// label. With them the decompiler reads `this->fields.moveRange` where it
// read an offset.
//
// Run headless by scripts/decomp/ghidra.py, after ApplyIl2Cpp.java:
// `-postScript ApplyIl2CppTypes.java <script.json> <il2cpp_ghidra.h>`, the
// header as ghidra.py rewrites it for Ghidra's C parser.
// @category mechcore

import java.io.FileInputStream;
import java.io.FileReader;
import java.io.InputStream;
import java.util.ArrayList;
import java.util.List;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import ghidra.app.cmd.function.ApplyFunctionSignatureCmd;
import ghidra.app.script.GhidraScript;
import ghidra.app.util.cparser.C.CParser;
import ghidra.app.util.cparser.C.CParserUtils;
import ghidra.program.model.address.Address;
import ghidra.program.model.data.DataType;
import ghidra.program.model.data.DataTypeManager;
import ghidra.program.model.data.FunctionDefinitionDataType;
import ghidra.program.model.data.PointerDataType;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.SourceType;

public class ApplyIl2CppTypes extends GhidraScript {

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length != 2) {
            throw new IllegalArgumentException(
                "usage: ApplyIl2CppTypes.java <script.json> <il2cpp_ghidra.h>");
        }
        DataTypeManager types = currentProgram.getDataTypeManager();

        monitor.setMessage("parsing " + args[1]);
        CParser parser = new CParser(types, true, null);
        try (InputStream header = new FileInputStream(args[1])) {
            parser.parse(header);
        }
        println("header parsed: " + types.getDataTypeCount(true) + " types");

        JsonObject root;
        try (FileReader reader = new FileReader(args[0])) {
            root = JsonParser.parseReader(reader).getAsJsonObject();
        }
        Address base = currentProgram.getImageBase();

        int signed = 0;
        List<String> unparsed = new ArrayList<>();
        var methods = root.getAsJsonArray("ScriptMethod");
        monitor.initialize(methods.size());
        monitor.setMessage("applying method signatures");
        for (JsonElement element : methods) {
            monitor.checkCancelled();
            monitor.incrementProgress(1);
            JsonObject method = element.getAsJsonObject();
            Function function = getFunctionAt(base.add(method.get("Address").getAsLong()));
            if (function == null) {
                continue;
            }
            String signature = method.get("Signature").getAsString();
            if (signature.endsWith(";")) {
                signature = signature.substring(0, signature.length() - 1);
            }
            FunctionDefinitionDataType definition;
            try {
                definition = CParserUtils.parseSignature(null, currentProgram, signature, false);
            } catch (Exception error) {
                definition = null;
            }
            if (definition == null) {
                if (unparsed.size() < 20) {
                    unparsed.add(signature);
                }
                continue;
            }
            // Folded methods share a body; the function keeps its own name.
            definition.setName(function.getName());
            ApplyFunctionSignatureCmd command = new ApplyFunctionSignatureCmd(
                function.getEntryPoint(), definition, SourceType.IMPORTED);
            if (command.applyTo(currentProgram, monitor)) {
                signed++;
            }
        }
        println("signatures applied: " + signed + " of " + methods.size());
        for (String signature : unparsed) {
            println("unparsed: " + signature);
        }

        int typed = 0;
        var metadata = root.getAsJsonArray("ScriptMetadata");
        monitor.initialize(metadata.size());
        monitor.setMessage("typing metadata");
        for (JsonElement element : metadata) {
            monitor.checkCancelled();
            monitor.incrementProgress(1);
            JsonObject entry = element.getAsJsonObject();
            JsonElement signature = entry.get("Signature");
            if (signature == null || signature.isJsonNull()) {
                continue;
            }
            DataType type = resolve(types, signature.getAsString());
            if (type == null) {
                continue;
            }
            Address address = base.add(entry.get("Address").getAsLong());
            try {
                clearListing(address, address.add(type.getLength() - 1));
                createData(address, type);
                typed++;
            } catch (Exception error) {
                // Another label's data already covers it.
            }
        }
        println("metadata typed: " + typed + " of " + metadata.size());
    }

    /** `Name*` as a pointer to the header's `Name`, or `Name` itself. */
    private static DataType resolve(DataTypeManager types, String signature) {
        String name = signature.trim();
        int pointers = 0;
        while (name.endsWith("*")) {
            name = name.substring(0, name.length() - 1).trim();
            pointers++;
        }
        List<DataType> found = new ArrayList<>();
        types.findDataTypes(name, found);
        if (found.size() != 1) {
            return null;
        }
        DataType type = found.get(0);
        for (int level = 0; level < pointers; level++) {
            type = new PointerDataType(type, types);
        }
        return type;
    }
}
