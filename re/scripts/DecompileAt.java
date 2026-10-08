// Create functions at the given addresses (if missing) and print their
// decompilation, plus any functions they call that are listed with a '+'
// suffix (e.g. 11c8f8+ decompiles callees one level deep).
// Usage: -postScript DecompileAt.java <outFile> <addr>[+] ...
// @category MGS2

import java.io.*;
import java.util.*;
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.*;

public class DecompileAt extends GhidraScript {
	@Override
	protected void run() throws Exception {
		String[] args = getScriptArgs();
		DecompInterface di = new DecompInterface();
		di.openProgram(currentProgram);
		LinkedHashSet<Function> todo = new LinkedHashSet<>();
		for (int i = 1; i < args.length; i++) {
			boolean deep = args[i].endsWith("+");
			Address a = toAddr(Long.parseLong(args[i].replace("+", ""), 16));
			Function f = getFunctionContaining(a);
			if (f == null) {
				disassemble(a);
				f = createFunction(a, null);
			}
			if (f == null) { println("no function at " + a); continue; }
			todo.add(f);
			if (deep) todo.addAll(f.getCalledFunctions(monitor));
		}
		try (PrintWriter w = new PrintWriter(new FileWriter(args[0]))) {
			for (Function f : todo) {
				DecompileResults r = di.decompileFunction(f, 60, monitor);
				w.println("// ==== " + f.getEntryPoint() + " " + f.getName());
				w.println(r != null && r.decompileCompleted() ? r.getDecompiledFunction().getC() : "// failed");
			}
		}
		di.dispose();
	}
}
