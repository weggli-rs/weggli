"""
 Copyright 2021 Google LLC

 Licensed under the Apache License, Version 2.0 (the "License");
 you may not use this file except in compliance with the License.
 You may obtain a copy of the License at

      https://www.apache.org/licenses/LICENSE-2.0

 Unless required by applicable law or agreed to in writing, software
 distributed under the License is distributed on an "AS IS" BASIS,
 WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 See the License for the specific language governing permissions and
 limitations under the License.
 """

import idautils
import ida_funcs
import ida_hexrays
import ida_lines
import idc

import weggli


def get_function(name):
    for f in idautils.Functions():
        if idc.get_func_name(f) == name:
            return f
    return None


def decompile(ea):
    func = ida_funcs.get_func(ea)
    if func == None:
        return None
    try:
        cfunc = ida_hexrays.decompile(func)
    except:
        return None

    if not cfunc:
        return None

    lines = [ida_lines.tag_remove(s.line) for s in cfunc.get_pseudocode()]
    return "\n".join(lines)


def xrefs_to(f):
    for xref in idautils.XrefsTo(f):
        f = ida_funcs.get_func(xref.frm)
        if f:
            yield f.start_ea


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
                        f"{len(results)} matches in {ida_funcs.get_func_name(target)}" + "  @ %08x" % target)
                    for r in results:
                        print(weggli.display(r, code))

            else:
                print(
                    f"Decompilation failed for {ida_funcs.get_func_name(target)}. Skipping..")
