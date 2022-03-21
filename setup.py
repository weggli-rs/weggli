from setuptools import setup
from setuptools_rust import Binding, RustExtension

setup(
    name="weggli",
    version="0.2.4",
    author="fwilhelm",
    url="https://github.com/googleprojectzero/weggli",
    rust_extensions=[RustExtension("weggli", binding=Binding.PyO3, features=["python"])],
    zip_safe=False,
)
