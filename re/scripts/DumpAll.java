// Headless: decompile every function into one C file and index string references.
// Usage: -postScript DumpAll.java <outDir>
// @category MGS2

import java.io.*;
import java.util.*;
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.data.StringDataInstance;
import ghidra.program.model.symbol.*;

public class DumpAll extends GhidraScript {
	@Override
	protected void run() throws Exception {
		String[] args = getScriptArgs();
		File out = new File(args.length > 0 ? args[0] : ".");
		out.mkdirs();

		DecompInterface di = new DecompInterface();
		di.openProgram(currentProgram);
		try (PrintWriter w = new PrintWriter(new FileWriter(new File(out, "all.c")))) {
			for (Function f : currentProgram.getFunctionManager().getFunctions(true)) {
				if (monitor.isCancelled()) break;
				DecompileResults r = di.decompileFunction(f, 60, monitor);
				w.println("// ==== " + f.getEntryPoint() + " " + f.getName());
				if (r != null && r.decompileCompleted()) {
					w.println(r.getDecompiledFunction().getC());
				} else {
					w.println("// decompile failed");
				}
			}
		}
		di.dispose();

		// string -> referencing functions
		try (PrintWriter w = new PrintWriter(new FileWriter(new File(out, "strings.txt")))) {
			for (Data d : currentProgram.getListing().getDefinedData(true)) {
				if (!d.hasStringValue()) continue;
				String s = StringDataInstance.getStringDataInstance(d).getStringValue();
				if (s == null) continue;
				StringBuilder refs = new StringBuilder();
				for (Reference ref : getReferencesTo(d.getAddress())) {
					Function f = getFunctionContaining(ref.getFromAddress());
					refs.append(' ').append(f == null ? ref.getFromAddress().toString() : f.getName());
				}
				w.println(d.getAddress() + "\t" + s.replace("\n", "\\n") + "\t" + refs);
			}
		}
	}
}
