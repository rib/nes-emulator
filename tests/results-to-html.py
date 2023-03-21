#!/usr/bin/env python3

import json
import textwrap
import argparse

parser = argparse.ArgumentParser(
        description='Convert JSON test results to HTML')
parser.add_argument('tests', type=str, help='Path to JSON test data')
parser.add_argument('results', type=str, help='Path to JSON results')
parser.add_argument('-o', '--output', type=str, help='HTML file to write')
args = parser.parse_args()

tag_precedence = [ "apu", "input", "mapper", "ppu", "cpu" ]

sections = {
    "cpu": { "name": "CPU Tests", "results": [] },
    "ppu": { "name": "PPU Tests", "results": [] },
    "apu": { "name": "APU Tests", "results": [] },
    "mapper": { "name": "Mapper Tests", "results": [] },
    "input": { "name": "Input Tests", "results": [] },
    "misc": { "name": "Misc Tests", "results": [] },
}

result_labels = {
    "PASSED": { "label": "Pass", "fg": "black", "bg": "lightgreen" },
    "FAILED": { "label": "Failed", "fg": "black", "bg": "pink" },
    "EXPECTED_FAILURE": { "label": "Failed (expected)", "fg": "black", "bg": "pink" },
    "UNKNOWN": { "label": "Unknown", "fg": "black", "bg": "yellow" }
}

with open(args.tests) as f:
    tests = json.load(f)

tests_by_name = {}
for test in tests:
    tests_by_name[test['name']] = test


with open(args.results) as f:
    results = json.load(f)

for result in results:
    test = tests_by_name[result['name']]
    result['test'] = test
    in_section = False
    for tag in tag_precedence:
        if tag in test['tags']:
            in_section = True
            sections[tag]['results'].append(result)
            break
    if not in_section:
        sections['misc']['results'].append(result)

tables_html = ''
for id in sections:
    section = sections[id]

    if len(section['results']) == 0:
        continue

    tables_html += '<h2>' + section["name"] + '</h2>\n'
    table_html = '<table><tr><th>Test</th><th>Result</th></tr>\n'

    for result in section['results']:

        label_info = result_labels[result['result']]
        bg_color = label_info['bg']
        fg_color = label_info['fg']
        label = label_info['label']
        table_html += f'    <tr><td>{result["name"]}</td><td style="background-color: {bg_color}; foreground-color: {fg_color};">{label}</td></tr>\n'

    table_html += '</table>\n'
    tables_html += table_html

html = '''
<html lang="en">
    <head>
    </head>
    <body>
{tables}
    </body>
</html>
'''.format(tables=textwrap.indent(tables_html, '        '))

if args.output:
    with open(args.output, 'w') as f:
        f.write(html)
else:
    print(html)
