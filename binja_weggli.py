from binaryninja import *

import weggli


def get_function(name):
	for f in bv.functions:
		if f.name == name:
			return f
	return None


def decompile(func):
    Settings().set_string('rendering.hlil.scopingStyle', 'bracesNewLine')
    settings = DisassemblySettings()
    settings.set_option(DisassemblyOption.ShowAddress, False)	
    settings.set_option(DisassemblyOption.WaitForIL, True)
    
    obj = lineardisassembly.LinearViewObject.language_representation(bv, settings)
    cursor_end = lineardisassembly.LinearViewCursor(obj)
    cursor_end.seek_to_address(func.highest_address)
    end_lines = bv.get_next_linear_disassembly_lines(cursor_end)
    cursor_end.seek_to_address(func.highest_address)
    start_lines = bv.get_previous_linear_disassembly_lines(cursor_end)
    lines = start_lines + end_lines
    
    return "\n".join(
        "".join(
            str(token)
            for token in line.contents.tokens
            if token.type != InstructionTextTokenType.TagToken
        )
        for line in lines
    )


def xrefs_to(f):
    for xref in bv.get_callers(f.start):
        f = bv.get_function_at(xref.address)
        if f:
            yield f


def run_query(query):
    qt = weggli.parse_query(query)

    identifiers = weggli.identifiers(qt)
    referenced_funcs = list(filter(lambda f: f != None, [
        get_function(i) for i in identifiers]))

    if len(referenced_funcs) > 0:
        work = set(xrefs_to(referenced_funcs[0]))

        for f in referenced_funcs[1:]:
            work.intersection_update(
                xrefs_to(f)
            )

        print(f"Searching through {len(work)} functions..")
        for target in work:
            if not target:
                continue
            code = decompile(target)
            if code != None:
                results = weggli.matches(qt, code)
                if len(results) > 0:
                    print(
                        f"{len(results)} matches in {target.name}" + "  @ %08x" % target.start)
                    for r in results:
                        print(weggli.display(r, code))

            else:
                print(
                    f"Decompilation failed for {target.name}. Skipping..")
