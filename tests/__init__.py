import unittest
import weggli
import typing


def parse_and_match(query, code) -> typing.List[str]:
    qt = weggli.parse_query(query)
    return [weggli.display(r, code) for r in weggli.matches(qt, code)]


class TestPythonBindings(unittest.TestCase):
    def test_match(self):
        results = parse_and_match(
            "{int $a = _+foo+$a;}", "void foo() {int bar=10+foo+bar;}"
        )
        self.assertEqual(
            results,
            [
                "void foo() {\x1b[31mint\x1b[0m \x1b[31mbar\x1b[0m=10+\x1b[31mfoo\x1b[0m+\x1b[31mbar\x1b[0m;}"
            ],
        )

    def test_identifiers(self):
        needle = "{int x = func(bar); xonk(foo);}"
        tree = weggli.parse_query(needle)
        identifiers = weggli.identifiers(tree)
        self.assertEqual(identifiers, ["int", "x", "func", "bar", "xonk", "foo"])
