import importlib.util
from pathlib import Path
import unittest
spec=importlib.util.spec_from_file_location('audit',Path(__file__).parents[1]/'audit-capture-dates.py')
audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)
class CaptureAuditTests(unittest.TestCase):
    def test_same_instant_in_different_timezones_is_not_repaired(self):
        aid='a'*64
        item={'filename':'BD_'+aid+'.jpg','label':'Photo - Portrait - Sep 12, 2026, 8:08:05 PM','detail_date':'Sep 12 Time taken: Yesterday, 8:08 PM GMT-07:00'}
        # Derive the independent UTC reference, not the function under test.
        import datetime as dt
        expected=int(dt.datetime(2026,9,13,3,8,5,tzinfo=dt.timezone.utc).timestamp()*1000)
        self.assertEqual(audit.classify_cloud(item,{aid:{'capture_ms':expected}})['status'],'correct')
        self.assertEqual(audit.classify_cloud(item,{aid:{'capture_ms':expected-86400000}})['status'],'date_mismatch')
        item['detail_date']='Sep 12';self.assertEqual(audit.classify_cloud(item,{aid:{'capture_ms':expected}})['status'],'unknown')
    def test_iso_requires_timezone_and_unloaded_panel_is_reported(self):
        self.assertIsNone(audit.cloud_instant({'observed_iso':'2026-09-14T10:00:00'}))
        self.assertIsNone(audit.cloud_instant({'observed_iso':'bad'}))
        self.assertEqual(audit.cloud_instant({'observed_iso':'2026-09-14T10:00:00+08:00'}), audit.cloud_instant({'observed_iso':'2026-09-14T02:00:00+00:00'}))
        self.assertEqual(audit.classify_cloud({'filename':None}, {})['status'],'unreadable')
    def test_unrecognized_file_and_missing_source_date_are_never_repaired(self):
        self.assertEqual(audit.classify_cloud({'filename':'IMG_0001.JPG'}, {})['status'],'unmatched')
        aid='b'*64
        self.assertEqual(audit.classify_cloud({'filename':'BD_'+aid+'.heic'},{aid:{'capture_ms':None}})['status'],'unknown')
    def test_receipt_maps_original_names_and_keeps_legacy_names(self):
        import json, tempfile
        aid,other='c'*64,'d'*64
        with tempfile.TemporaryDirectory() as tmp:
            receipt=Path(tmp)/'receipt.json'
            receipt.write_text(json.dumps({'schema_version':1,'items':[
                {'id':aid,'name':'IMG_1234.HEIC'},{'id':other,'name':'IMG_1234 (1).HEIC'},
                {'id':aid,'name':'reused.jpg'},{'id':other,'name':'reused.jpg'},{'id':'bad','name':'bad.jpg'}]}))
            names=audit.read_receipt(receipt)
            media=Path(tmp)/'media.txt'
            media.write_text('Row: 0 _id=7, _display_name=IMG_1234 (1).HEIC, datetaken=1, date_added=2, date_modified=3\n'
                'Row: 1 _id=8, _display_name=unknown.jpg, datetaken=1, date_added=2, date_modified=3\n')
            self.assertEqual([r['asset_id'] for r in audit.read_media(media,names)],[other])
        self.assertEqual(names['IMG_1234.HEIC'],aid)
        self.assertIsNone(names['reused.jpg'])
        self.assertNotIn('bad.jpg',names)
        assets={aid:{'capture_ms':None},other:{'capture_ms':None}}
        self.assertEqual(audit.classify_cloud({'filename':'IMG_1234.HEIC'},assets,names)['asset_id'],aid)
        self.assertEqual(audit.classify_cloud({'filename':'reused.jpg'},assets,names)['status'],'unmatched')
        self.assertEqual(audit.classify_cloud({'filename':'IMG_1234.HEIC'},assets)['status'],'unmatched')
        self.assertEqual(audit.classify_cloud({'filename':'BD_'+aid+'.heic'},assets,names)['asset_id'],aid)
if __name__=='__main__': unittest.main()
