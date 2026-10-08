// Map a stage's resident code image (STAGE.DAT page kind 0x7F010000, type-1
// file) at its load address and label its script-command table.
// Usage: -postScript LoadResident.java <blob path>
// @category MGS2

import java.io.*;
import java.nio.file.*;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.mem.*;
import ghidra.program.model.symbol.SourceType;

public class LoadResident extends GhidraScript {
	static final long BASE = 0x1F1480L;

	@Override
	protected void run() throws Exception {
		byte[] blob = Files.readAllBytes(Paths.get(getScriptArgs()[0]));
		Address base = toAddr(BASE);
		Memory mem = currentProgram.getMemory();
		MemoryBlock existing = mem.getBlock("resident");
		if (existing != null) mem.removeBlock(existing, monitor);
		MemoryBlock b = mem.createInitializedBlock("resident", base,
			new ByteArrayInputStream(blob), blob.length, monitor, false);
		b.setRead(true); b.setWrite(true); b.setExecute(true);

		// Header: [load addr, code/data end, bss end, table ptr]; table is
		// {u32 name_hash, u32 func} pairs terminated by hash 0.
		long end = mem.getInt(toAddr(BASE + 4)) & 0xFFFFFFFFL;
		long bssEnd = mem.getInt(toAddr(BASE + 8)) & 0xFFFFFFFFL;
		long bssStart = BASE + blob.length;
		if (bssEnd > bssStart) {
			MemoryBlock bss = mem.getBlock("resident.bss");
			if (bss != null) mem.removeBlock(bss, monitor);
			mem.createUninitializedBlock("resident.bss", toAddr(bssStart), bssEnd - bssStart, false);
		}
		printf("resident: %08x-%08x (header end %08x, bss end %08x)%n", BASE, BASE + blob.length, end, bssEnd);

		long table = mem.getInt(toAddr(BASE + 12)) & 0xFFFFFFFFL;
		int n = 0;
		for (long p = table; ; p += 8) {
			long hash = mem.getInt(toAddr(p)) & 0xFFFFFFFFL;
			if (hash == 0) break;
			Address fn = toAddr(mem.getInt(toAddr(p + 4)) & 0xFFFFFFFFL);
			String name = String.format("cmd_%06x", hash);
			disassemble(fn);
			if (getFunctionAt(fn) == null) createFunction(fn, name);
			else if (getFunctionAt(fn).getName().startsWith("FUN_"))
				getFunctionAt(fn).setName(name, SourceType.USER_DEFINED);
			createLabel(toAddr(p), String.format("cmdtab_%06x", hash), true);
			n++;
		}
		printf("labelled %d script command handlers%n", n);
	}
}
