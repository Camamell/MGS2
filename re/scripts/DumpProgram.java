// Headless: decompile every function of the current program into one file.
// Usage: -postScript DumpProgram.java <outFile>
// @category MGS2

import java.io.*;
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;

public class DumpProgram extends GhidraScript {
	@Override
	protected void run() throws Exception {
		DecompInterface d = new DecompInterface();
		d.openProgram(currentProgram);
		try (PrintWriter out = new PrintWriter(new FileWriter(getScriptArgs()[0]))) {
			for (Function f : currentProgram.getFunctionManager().getFunctions(true)) {
				DecompileResults r = d.decompileFunction(f, 60, monitor);
				out.println("// ==== " + f.getEntryPoint() + " " + f.getName());
				out.println(r.decompileCompleted() ? r.getDecompiledFunction().getC() : "// failed");
			}
		}
	}
}
