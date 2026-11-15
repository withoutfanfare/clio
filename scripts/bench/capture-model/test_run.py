"""Offline checks for the benchmark judge; no model calls."""
import unittest
from run import judge


class CeilingTests(unittest.TestCase):
    def test_pre_cap_non_explicit_count_catches_trimmed_overrun(self):
        self.assertTrue(any("ceiling ignored" in failure for failure in judge("other", [], 4)))
        self.assertFalse(any("ceiling ignored" in failure for failure in judge("other", [], 3)))

    def test_explicit_commitments_may_extend_stored_total(self):
        knowledge = [{"kind": "fact"}] * 3
        explicit = [{"kind": "task", "attention": {"explicitness": "explicit"}}] * 2
        self.assertEqual(judge("other", knowledge + explicit, 3), [])


if __name__ == "__main__":
    unittest.main()
