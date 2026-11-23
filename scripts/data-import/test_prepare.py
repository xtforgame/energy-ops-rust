import unittest
from prepare import normalize_meter, consolidate

class ImportTests(unittest.TestCase):
    def test_meter_preserves_leading_zero(self):
        self.assertEqual(normalize_meter('03-43-2888-88-6'), '03432888886')
        self.assertIsNone(normalize_meter('N/A'))
        self.assertIsNone(normalize_meter('03-43-2888-88-6 / 20-72-7017-93-7'))

    def test_same_meter_deduplicated_conflicts_not_guessed(self):
        a={'meterKey':'03432888886','plantNo':'A','name':'甲','sourceRow':1}
        rows, conflicts=consolidate([a,dict(a,sourceRow=2)])
        self.assertEqual(len(rows),1)
        self.assertFalse(conflicts)
        rows, conflicts=consolidate([a,dict(a,plantNo='B',name='乙')])
        self.assertEqual(rows,[])
        self.assertEqual(len(conflicts),1)

if __name__=='__main__':unittest.main()
