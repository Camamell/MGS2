// Print the call tree under a function: address, name, size in bytes,
// each function expanded once.
// Usage: -postScript CallTree.java <outFile> <addr> <maxDepth>
// @category MGS2

import java.io.*;
import java.util.*;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;

public class CallTree extends GhidraScript {
	PrintWriter w;
	Set<Function> seen = new HashSet<>();
	int maxDepth;
	long total = 0;

	@Override
	protected void run() throws Exception {
		String[] a = getScriptArgs();
		maxDepth = Integer.parseInt(a[2]);
		Function root = getFunctionContaining(toAddr(Long.parseLong(a[1], 16)));
		try (PrintWriter pw = new PrintWriter(new FileWriter(a[0]))) {
			w = pw;
			walk(root, 0);
			w.println("// distinct functions: " + seen.size() + ", bytes: " + total);
		}
	}

	void walk(Function f, int depth) throws Exception {
		boolean first = seen.add(f);
		long size = f.getBody().getNumAddresses();
		if (first) total += size;
		w.println("  ".repeat(depth) + f.getEntryPoint() + " " + f.getName() + " " + size + (first ? "" : " *"));
		if (!first || depth >= maxDepth) return;
		List<Function> callees = new ArrayList<>(f.getCalledFunctions(monitor));
		callees.sort(Comparator.comparing(Function::getEntryPoint));
		for (Function c : callees) walk(c, depth + 1);
	}
}
