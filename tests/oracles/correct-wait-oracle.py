from pathlib import Path
import shutil
root = Path(".codex/wait-differential/corrected")
source = Path("third_party/rcv/data/waitcnt")
root.mkdir(parents=True, exist_ok=True)
for path in source.glob("*"):
    shutil.copyfile(path, root / path.name)
path = root / "analysis.cpp"
text = path.read_text()
needle = '    size_t iter = pos + name.size() + 1;'
assert text.count(needle) == 1
text = text.replace(needle, needle + '''
    if (name == "vscnt" && str.find(',') != std::string::npos)
        return std::stoll(str.substr(str.rfind(',') + 1), nullptr, 0);
''')
path.write_text(text)
for filename, counter in [("gfx10.cpp", "lgkm"), ("gfx12.cpp", "dscnt")]:
    path = root / filename
    text = path.read_text()
    start = text.index(f"                if (auto joined = {counter}.handle_mem_op(inst_str, flat_load))")
    end = text.index("\n            }", start)
    text = text[:start] + f'''                auto joined = {counter}.handle_mem_op(inst_str, flat_load).value_or(std::vector<int>{{}});
                if (!flat_stor.empty())
                    if (auto stor = {counter}.handle_mem_op(inst_str, flat_stor))
                        joined.insert(joined.end(), stor->begin(), stor->end());
                if (!joined.empty())
                    mem_unroll.emplace_back(LineWaitcnt{{line_number, std::move(joined)}});
''' + text[end:]
    path.write_text(text)
