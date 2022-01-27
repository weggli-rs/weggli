import unittest
import weggli
import typing
import os


def parse_and_match(query, code, cpp=False, color=False) -> typing.List[str]:
    qt = weggli.parse_query(query, cpp)
    return [weggli.display(r, code, color) for r in weggli.matches(qt, code, cpp)]


class TestPythonBindings(unittest.TestCase):
    def test_match(self):
        results = parse_and_match(
            "{int $a = _+foo+$a;}", "void foo() {int bar=10+foo+bar;}"
        )
        self.assertEqual(
            results,
            ["void foo() {int bar=10+foo+bar;}"],
        )

    def test_color(self):
        results = parse_and_match(
            "{int $a = _+foo+$a;}", "void foo() {int bar=10+foo+bar;}", color=True
        )
        self.assertEqual(
            results,
            [
                "void foo() {\x1b[31mint\x1b[0m "
                "\x1b[31mbar\x1b[0m=10+\x1b[31mfoo\x1b[0m+\x1b[31mbar\x1b[0m;}"
            ],
        )

    def test_identifiers(self):
        needle = "{int x = func(bar); xonk(foo);}"
        tree = weggli.parse_query(needle)
        identifiers = weggli.identifiers(tree)
        self.assertEqual(identifiers, ["int", "x", "func", "bar", "xonk", "foo"])

    def test_cpp(self):
        code = """
        #include <iostream>\n
        int main() {
        std::cout << "Hello World!";
        return 0;
        }
        """
        results = parse_and_match(
            "_ $func() {std::cout << _;}", code, cpp=True, color=False
        )
        self.assertEqual(
            results,
            [
                "int main() {\n"
                '        std::cout << "Hello World!";\n'
                "        return 0;\n"
                "        }"
            ],
        )
