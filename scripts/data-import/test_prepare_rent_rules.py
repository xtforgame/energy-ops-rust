import importlib.util
from pathlib import Path
import unittest
import tempfile
import openpyxl

spec = importlib.util.spec_from_file_location('rent', Path(__file__).with_name('prepare-rent-rules.py'))
rent = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rent)

class SourceFormulaTests(unittest.TestCase):
    def test_fixed_annual_header_is_not_cycle(self):
        r = rent.extract_formula({'AW':120000,'BC':'六個月'}, annual_header=True)
        self.assertEqual((r['fixedAmount'],r['fixedUnit']), (120000,'year'))
        self.assertEqual(r['cycleMonths'],6)
    def test_unknown_header_preserves_reference(self):
        r = rent.extract_formula({'AW':120000,'BC':'六個月'})
        self.assertIsNone(r['fixedUnit'])
        self.assertIn('fixedUnit',r['sourceMissing'])
    def test_floor_gross_not_untaxed(self):
        r = rent.extract_formula({'BA':'12%(含稅低於1萬以含稅1萬計)','AY':'X','BC':'一個月'})
        self.assertEqual((r['calculationType'],r['minimumRent'],r['minimumBasis']),('percentageFloor',10000,'gross'))
        self.assertIn('taxPercent',r['sourceMissing'])
    def test_school_not_simple_percent(self):
        r = rent.extract_formula({'BA':.1,'BB':'123,456/年','AY':'V'})
        self.assertEqual(r['calculationType'],'annualReference')
        self.assertEqual(r['annualRent'],123456)
        self.assertIn('annualAdjustmentMethod',r['sourceMissing'])
    def test_area_no_invented_conversion(self):
        r = rent.extract_formula({'AS':.067,'AU':'4萬/每分地'},annual_header=True)
        self.assertEqual(r['unitRate'],40000)
        self.assertIn('areaUnitConversion',r['sourceMissing'])
    def test_two_month_and_unknown_floor(self):
        r = rent.extract_formula({'BA':.06,'AY':'X','BC':'二個月','BB':'有最低應繳金額，提供最低收益計算公式'})
        self.assertEqual(r['cycleMonths'],2)
        self.assertEqual(r['calculationType'],'percentageFloor')
        self.assertIn('minimumRent',r['sourceMissing'])
    def test_owner_annual_alternative_avoids_conversion(self):
        r=rent.extract_formula({'AS':.1,'AU':'4萬/每分地'},True,[{'amount':40000,'paymentSchedule':'每年'}])
        self.assertEqual((r['calculationType'],r['fixedAmount'],r['fixedUnit']),('fixed',40000,'year'))
        self.assertNotIn('areaUnitConversion',r['sourceMissing'])
    def test_conflicting_per_payment_amount_never_assumed_annual(self):
        r=rent.extract_formula({'AW':30000,'BC':'三個月'},True,[{'amount':15000},{'amount':15000}])
        self.assertIsNone(r['fixedUnit'])
    def test_shared_reference_and_confirmed_override(self):
        with tempfile.TemporaryDirectory() as tmp:
            w=openpyxl.Workbook();s=w.active;s.title='0. 資產及義務項目清單'
            s['AS1']='土地租金(年)';s.merge_cells('AS1:AW1')
            for row,project,amount in [(4,'P1',120000),(5,'P2','同上合併計算')]:
                for col,value in {'C':project,'R':str(10000000000+row),'W':'共同收款者','AW':amount,'BC':'一年'}.items(): s[f'{col}{row}']=value
            path=Path(tmp)/'asset.xlsx';w.save(path)
            stations=[{'id':i,'name':str(i),'customerNumber':str(10000000000+i)} for i in (4,5)]
            result=rent.prepare(path,stations,{})
            self.assertTrue(all(x['rule']['groupAllocationMissing'] for x in result['items']))
            self.assertTrue(result['items'][1]['rule']['sharedAmountReferenceOnly'])
            result=rent.prepare(path,stations,{'P1|10000000004':{'status':'confirmed','calculationType':'percentage','percent':'9'}})
            self.assertEqual(result['items'][0]['rule']['percent'],'9')

if __name__ == '__main__': unittest.main()
