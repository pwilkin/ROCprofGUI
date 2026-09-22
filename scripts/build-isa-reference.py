#!/usr/bin/env python3
"""Generate the offline reference from AMD's MIT-licensed XML specification ZIP."""
import argparse
import hashlib
import html
import json
from pathlib import Path
import re
import xml.etree.ElementTree as ET
import zipfile

ARCHITECTURES = ['cdna1', 'cdna2', 'cdna3', 'cdna4', 'rdna1', 'rdna2', 'rdna3', 'rdna3_5', 'rdna4']
SOURCE = 'https://gpuopen.com/machine-readable-isa/'
DOWNLOAD = 'https://gpuopen.com/download/machine-readable-isa/latest/'
MANUALS = 'https://gpuopen.com/amd-gpu-architecture-programming-documentation/'


def value(node, path):
    text = node.findtext(path, '')
    text = re.sub(r'</?(?:p|br|div|li|ul|ol|pre|code|b|i|em|strong|sup|sub|span|a|table|tr|td|th)\b[^>]*>', ' ', text)
    return re.sub(r'[ \t]+', ' ', html.unescape(text)).strip()


def matrix(name):
    match = re.fullmatch(r'V_(WMMA|SWMMAC|MFMA|SMFMAC)(_SCALE)?_([A-Z0-9]+)_(\d+X\d+X\d+)(?:_(\d+B))?_?([A-Z].+)', name)
    if not match:
        return None
    family, scale, accumulator, shape, blocks, inputs = match.groups()
    return dict(family=family, scaled=bool(scale), accumulator=accumulator, shape=shape.lower(), blocks=blocks or '', inputs=inputs)


def build(archive, output):
    raw = Path(archive).read_bytes()
    output.mkdir(parents=True, exist_ok=True)
    manifest = dict(version=1, source=SOURCE, download=DOWNLOAD, manuals=MANUALS,
                    archiveSha256=hashlib.sha256(raw).hexdigest(), architectures=[])
    with zipfile.ZipFile(archive) as z:
        for arch in ARCHITECTURES:
            filename = f'amdgpu_isa_{arch}.xml'
            xml = z.read(filename)
            root = ET.fromstring(xml)
            assert value(root, './Document/License') == 'MIT', 'Unexpected source license'
            instructions = []
            for node in root.findall('./ISA/Instructions/Instruction'):
                name = value(node, 'InstructionName')
                encodings = []
                for enc in node.findall('./InstructionEncodings/InstructionEncoding'):
                    operands = [dict(field=value(o, 'FieldName'), format=value(o, 'DataFormatName'),
                                     type=value(o, 'OperandType'), bits=value(o, 'OperandSize'),
                                     input=o.get('Input') == 'true', output=o.get('Output') == 'true',
                                     implicit=o.get('IsImplicit') == 'true')
                                for o in enc.findall('./Operands/Operand')]
                    opcode = enc.find('Opcode')
                    encodings.append(dict(name=value(enc, 'EncodingName'), condition=value(enc, 'EncodingCondition'),
                                          opcode=value(enc, 'Opcode'), radix=opcode.get('Radix', '10') if opcode is not None else '10', operands=operands))
                instructions.append(dict(name=name, description=value(node, 'Description'), group=value(node, './FunctionalGroup/Name'),
                                         subgroups=[s.text for s in node.findall('./FunctionalGroup/FunctionalSubgroups/Subgroup')],
                                         flags=[f.tag for f in node.findall('./InstructionFlags/*') if f.text == 'TRUE'],
                                         encodings=encodings, matrix=matrix(name)))
            assert all(i['matrix'] for i in instructions if re.match(r'V_(?:S?MFMA|S?WMMA)', i['name'])), 'Unparsed matrix instruction'
            instructions.sort(key=lambda i: i['name'])
            assert len({i['name'] for i in instructions}) == len(instructions)
            formats = {value(f, 'DataFormatName'): dict(description=value(f, 'Description'), type=value(f, 'DataType'),
                       bits=value(f, 'BitCount'), components=value(f, 'ComponentCount'))
                       for f in root.findall('./ISA/DataFormats/DataFormat')}
            encodings = {value(e, 'EncodingName'): dict(description=value(e, 'Description'), bits=value(e, 'BitCount'),
                         fields=[dict(name=value(f, 'FieldName'), description=value(f, 'Description'),
                                      ranges=[dict(offset=value(r, 'BitOffset'), bits=value(r, 'BitCount')) for r in f.findall('./BitLayout/Range')])
                                 for f in e.findall('./MicrocodeFormat/BitMap/Field')]) for e in root.findall('./ISA/Encodings/Encoding')}
            metadata = dict(id=arch, name=value(root, './ISA/Architecture/ArchitectureName'),
                            release=value(root, './Document/ReleaseDate'), schema=value(root, './Document/SchemaVersion'),
                            copyright=value(root, './Document/Copyright'), license='MIT', file=filename,
                            sha256=hashlib.sha256(xml).hexdigest(), count=len(instructions))
            data = dict(**metadata, instructions=instructions, formats=formats, encodings=encodings)
            (output / f'{arch}.json').write_text(json.dumps(data, ensure_ascii=False, separators=(',', ':')) + '\n')
            manifest['architectures'].append(dict(**metadata, mnemonics=[i['name'] for i in instructions],
                 matrix=[dict(name=i['name'], **i['matrix']) for i in instructions if i['matrix']]))
            print(arch, len(instructions), 'instructions;', len(manifest['architectures'][-1]['matrix']), 'matrix variants')
    (output / 'manifest.json').write_text(json.dumps(manifest, ensure_ascii=False, separators=(',', ':')) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    parser.add_argument('--output', type=Path, default=Path(__file__).resolve().parents[1] / 'public/isa')
    args = parser.parse_args()
    build(args.archive, args.output)
