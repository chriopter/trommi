import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock

spec = importlib.util.spec_from_file_location('startup_report', Path(__file__).resolve().parents[1] / 'UITests' / 'startup_report.py')
report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(report)


class Clock:
    def __init__(self): self.now = .006
    def __call__(self): return self.now
    def sleep(self, seconds): self.now += seconds


class StartupReportTests(unittest.TestCase):
    def test_only_the_launched_process_report_is_accepted(self):
        path = Mock()
        path.read_text.side_effect = [json.dumps({'process_id': 3}), json.dumps({'process_id': 7, 'rows': 290})]
        clock = Clock()
        self.assertEqual(report.wait_for_report(path, Mock(read_text=lambda: 'com.trommi.ios: 7\n'), Mock(poll=lambda: 0), clock, clock.sleep)[0]['rows'], 290)
        self.assertEqual(path.read_text.call_count, 2)

    def test_a_stale_report_cannot_satisfy_the_deadline(self):
        path = Mock()
        path.read_text.return_value = json.dumps({'process_id': 3})
        clock = Clock()
        with self.assertRaisesRegex(AssertionError, 'own startup report'):
            report.wait_for_report(path, Mock(read_text=lambda: 'com.trommi.ios: 7\n'), Mock(poll=lambda: 0), clock, clock.sleep, timeout=.02)

    def test_launch_is_included_in_the_same_process_clock_interval(self):
        clock = Clock()
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'report.json'
            def launch(*args, **kwargs):
                clock.sleep(.75)
                path.write_text(json.dumps({'process_id': 7, 'rows': 290, 'desk_ms': 600}))
                kwargs['stdout'].write('com.trommi.ios: 7\n'); kwargs['stdout'].flush()
                finish = clock() + .25
                return Mock(poll=lambda: None if clock() < finish else 0, wait=lambda **kw: 0)
            value = report.measure_launch('isolated-sim', Path('/fixture'), path, Path(folder) / 'launch.log', clock, clock.sleep, launch, wall=lambda:1000 + clock(), born=lambda pid:1000.1)
            self.assertAlmostEqual(value['launch_to_desk_ms'], 750)
            self.assertAlmostEqual(value['process_to_desk_ms'], 656)
            self.assertGreaterEqual(value['simctl_total_ms'], 1000)
            report.validate_report(value, 290, 2000)

    def test_wrong_rows_over_budget_and_impossible_time_are_refused(self):
        for rows, desk, wall in [(289, 600, 750), (290, 600, 2001), (290, 800, 750), (290, 0, 750)]:
            with self.subTest(rows=rows, desk=desk, wall=wall), self.assertRaises(AssertionError):
                report.validate_report({'rows': rows, 'desk_ms': desk, 'process_to_desk_ms': wall, 'launch_to_desk_ms': wall}, 290, 2000)

    def test_a_fast_process_does_not_hide_an_over_budget_cold_launch(self):
        value = {'rows':290, 'desk_ms':600, 'process_to_desk_ms':750, 'launch_to_desk_ms':4000}
        with self.assertRaises(AssertionError): report.validate_report(value, 290, 2000)
        value['launch_to_desk_ms'] = 1000
        report.validate_report(value, 290, 2000)
        value['process_to_desk_ms'] = 2001
        with self.assertRaises(AssertionError): report.validate_report(value, 290, 2000)

    def test_other_or_ambiguous_launches_are_refused(self):
        for text in ['another.app: 7', 'com.trommi.ios: 0', 'com.trommi.ios: 7\ncom.trommi.ios: 8']:
            with self.subTest(text=text), self.assertRaises(AssertionError): report.launched_pid(text)


if __name__ == '__main__': unittest.main()
