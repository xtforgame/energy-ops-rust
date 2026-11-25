import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch, Mock

spec = importlib.util.spec_from_file_location('prepare_rules', Path(__file__).with_name('prepare-rent-rules.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class PreparationTests(unittest.TestCase):
    def run_prepare(self, cells, stations, overrides=None, rows=4):
        class Sheet:
            max_row = rows
            title = '0. 資產及義務項目清單'
            def __getitem__(self, key):
                return Mock(value=cells.get(key))
        source = Mock()
        source.name = 'asset.xlsx'
        source.read_bytes.return_value = b'source-fingerprint'
        with patch('openpyxl.load_workbook', return_value={Sheet.title: Sheet()}):
            return module.prepare(source, stations, overrides or {})

    def test_known_percentage_still_pending_without_confirmed_override(self):
        result = self.run_prepare({'C4':'project','R4':'20-72-7017-93-7','BA4':0.08,'Y4':'2017-06-09'}, [{'id':70,'name':'Site','customerNumber':'20-72-7017-93-7'}])
        rule = result['items'][0]['rule']
        self.assertEqual(rule['status'], 'pending')
        self.assertEqual(rule['percent'], '8.0')
        self.assertIsNone(rule['validFrom'])
        self.assertEqual(rule['sourceValues']['Y'], '2017-06-09')

    def test_shared_meter_is_not_guessed(self):
        result = self.run_prepare({'C4':'project','R4':'20-72-7017-93-7'}, [{'id':1,'customerNumber':'20-72-7017-93-7'},{'id':2,'customerNumber':'20-72-7017-93-7'}])
        self.assertEqual(result['items'], [])
        self.assertEqual(len(result['unmatched']), 1)

    def test_override_requires_both_project_and_meter(self):
        cells={'C4':'project','R4':'20-72-7017-93-7'}
        stations=[{'id':70,'name':'Site','customerNumber':'20-72-7017-93-7'}]
        wrong=self.run_prepare(cells,stations,{'other|20727017937':{'status':'confirmed'}})
        self.assertEqual(wrong['items'][0]['rule']['status'],'pending')
        right=self.run_prepare(cells,stations,{'project|20727017937':{'status':'confirmed'}})
        self.assertEqual(right['items'][0]['rule']['status'],'confirmed')

if __name__ == '__main__':
    unittest.main()
