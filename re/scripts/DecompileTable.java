// Decompile every function referenced by a table of code pointers.
// Usage: -postScript DecompileTable.java <outFile> <tableAddr> <count>
// @category MGS2

import java.io.*;
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.*;

public class DecompileTable extends GhidraScript {
	@Override
	protected void run() throws Exception {
		String[] a = getScriptArgs();
		Address table = toAddr(Long.parseLong(a[1], 16));
		int count = Integer.parseInt(a[2]);
		DecompInterface di = new DecompInterface();
		di.openProgram(currentProgram);
		try (PrintWriter w = new PrintWriter(new FileWriter(a[0]))) {
			for (int i = 0; i < count; i++) {
				Address target = toAddr(getInt(table.add(i * 4L)) & 0xFFFFFFFFL);
				w.println("// ---- [" + i + "] -> " + target);
				Function f = getFunctionAt(target);
				if (f == null) {
					disassemble(target);
					f = createFunction(target, null);
				}
				if (f == null) {
					// Jump-table targets are often labels inside a function.
					Function c = getFunctionContaining(target);
					w.println("// label inside " + (c == null ? "?" : c.getName()));
					continue;
				}
				DecompileResults r = di.decompileFunction(f, 60, monitor);
				w.println(r != null && r.decompileCompleted() ? r.getDecompiledFunction().getC() : "// failed");
			}
		}
		di.dispose();
	}
}
