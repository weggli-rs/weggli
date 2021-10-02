# weggli

![weggli example](example.gif)

## Introduction
weggli is a fast and robust semantic search tool for C and C++ codebases.
It is designed to help security researchers identify interesting functionality in large codebases.

weggli performs pattern matching on Abstract Syntax Trees based on user provided queries. Its query language
resembles C and C++ code, making it easy to turn interesting code patterns into queries.

weggli is inspired by great tools like [Semgrep](https://semgrep.dev/), [Coccinelle](https://coccinelle), [joern](https://joern.readthedocs.io/en/latest/) and [CodeQL](https://securitylab.github.com/tools/codeql), but makes some different design decisions:

- **C++ support**: weggli has first class support for modern C++ constructs, such as lambda expressions, range-based for loops and constexprs. 

- **Minimal setup**: weggli should work *out-of-the box* against most software you will encounter. weggli does not require the ability to build the software and can work with incomplete sources or missing dependencies. 
  
- **Interactive**: weggli is designed for interactive usage and fast query performance. Most of the time, a weggli query will be faster than a grep search. The goal is to enable an interactive workflow where quick switching between code review and query creation/improvement is possible.
  
- **Greedy**: weggli's pattern matching is designed to find as many (useful) matches as possible for a specific query. While this increases the risk of false positives it simplifies query creation. For example, the query  `$x = 10;` will match both assignment expressions (`foo = 10;`) and declarations (`int bar = 10;`). 




## Usage
```
Use -h for short descriptions and --help for more details.

Homepage: https://github.com/googleprojectzero/weggli

USAGE: weggli [OPTIONS] <PATTERN> <PATH>

ARGS:
    <PATTERN>    
            A weggli search pattern. weggli's query language closely resembles
            C and C++ with a small number of extra features.
            
            For example, the pattern '{_ $buf[_]; memcpy($buf,_,_);}' will
            find all calls to memcpy that directly write into a stack buffer.
            
            Besides normal C and C++ constructs, weggli's query language
            supports the following features:
            
            _        Wildcard. Will match on any AST node. 
            
            $var     Variables. Can be used to write queries that are independent
                     of identifiers. Variables match on identifiers, types,
                     field names or namespaces. The --unique option
                     optionally enforces that $x != $y != $z. The --regex option can
                     enforce that the variable has to match (or not match) a
                     regular expression.
            
            _(..)    Subexpressions. The _(..) wildcard matches on arbitrary
                     sub expressions. This can be helpful if you are looking for some
                     operation involving a variable, but don't know more about it.
                     For example, _(test) will match on expressions like test+10,
                     buf[test->size] or f(g(&test));
            
            not:     Negative sub queries. Only show results that do not match the
                     following sub query. For example, '{not: $fv==NULL; not: $fv!=NULL *$v;}'
                     would find pointer dereferences that are not preceded by a NULL check.
            
            weggli automatically unwraps expression statements in the query source 
            to search for the inner expression instead. This means that the query `{func($x);}` 
            will match on `func(a);`, but also on `if (func(a)) {..}` or  `return func(a)`. 
            Matching on `func(a)` will also match on `func(a,b,c)` or `func(z,a)`. 
            Similarly, `void func($t $param)` will also match function definitions 
            with multiple parameters. 
            
            Additional patterns can be specified using the --pattern (-p) option. This makes
            it possible to search across functions or type definitions.
    <PATH>       
            Input directory or file to search. By default, weggli will search inside 
            .c and .h files for the default C mode or .cc, .cpp, .cxx, .h and .hpp files when
            executing in C++ mode (using the --cpp option).
            Alternative file endings can be specified using the --extensions (-e) option.
            
            When combining weggli with other tools or preprocessing steps, 
            files can also be specified via STDIN by setting the directory to '-' 
            and piping a list of filenames.

OPTIONS:
    -A, --after <after>                 
            Lines to print after a match. Default = 5.

    -B, --before <before>               
            Lines to print before a match. Default = 5.

    -C, --color                         
            Force enable color output.

    -X, --cpp                           
            Enable C++ mode.

        --exclude <exclude>...          
            Exclude files that match the given regex.

    -e, --extensions <extensions>...    
            File extensions to include in the search.

    -f, --force                         
            Force a search even if the queries contains syntax errors.

    -h, --help                          
            Prints help information.

        --include <include>...          
            Only search files that match the given regex.

    -l, --limit                         
            Only show the first match in each function.

    -p, --pattern <p>...                
            Specify additional search patterns.

    -R, --regex <regex>...              
            Filter variable matches based on a regular expression. 
            This feature uses the Rust regex crate, so most Perl-style
            regular expression features are supported.
            (see https://docs.rs/regex/1.5.4/regex/#syntax)
            
            Examples:
            
            Find calls to functions starting with the string 'mem':
            weggli -R 'func=^mem' '$func(_);'       
            
            Find memcpy calls where the last argument is NOT named 'size':
            weggli -R 's!=^size$' 'memcpy(_,_,$s);' 
    -u, --unique                        
            Enforce uniqueness of variable matches.
            By default, two variables such as $a and $b can match on identical values.
            For example, the query '$x=malloc($a); memcpy($x, _, $b);' would
            match on both
            
            void *buf = malloc(size);
            memcpy(buf, src, size);
            
            and
            
            void *buf = malloc(some_constant);
            memcpy(buf, src, size);
            
            Using the unique flag would filter out the first match as $a==$b.
    -v, --verbose                       
            Sets the level of verbosity.

    -V, --version                       
            Prints version information.
```

## Examples
Calls to memcpy that write into a stack-buffer:

```c
weggli '{
    _ $buf[_];
    memcpy($buf,_,_);
}' ./target/src
```

Potentially vulnerable snprintf() users:
```c
weggli '{
    $ret = snprintf($b,_,_);
    $b[$ret] = _;
}' ./target/src
```

Potentially uninitialized pointers:
```c
weggli '{ _* $p;
NOT: $p = _;
$func(&$p);
}' ./target/src
```

Potentially insecure WeakPtr usage:
```cpp
weggli -X '{
$x = _.GetWeakPtr(); 
DCHECK($x); 
$x->_;}' ./target/src
```

Debug only iterator validation:
```cpp
weggli -X 'DCHECK(_!=_.end());' ./target/src
```

Functions that perform writes into a stack-buffer based on
a function argument. 
```c
weggli '_ $fn(_ $limit) {
    _ $buf[_];
    for (_; $i<$limit; _) {
        $buf[$i]=_;
    }
}' ./target/src
```

Functions with the string decode in their name
```c
weggli -R func=decode '_ $func(_) {_;}'
```

Encoding/Conversion functions
```c
weggli '_ $func($t *$input, $t2 *$output) {
    for (_($i);_;_) {
        $input[$i]=_($output);
    }
}' ./target/src
```

## Build Instruction

```sh
# optional: install rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh 

git clone https://github.com/googleprojectzero/weggli.git
cd weggli; cargo build --release
./target/release/weggli
```

## Implementation details

Weggli is built on top of the [`tree-sitter`](https://tree-sitter.github.io/tree-sitter/) parsing library and its [`C`](https://github.com/tree-sitter/tree-sitter-c) and [`C++`](https://github.com/tree-sitter/tree-sitter-cpp) grammars.
Search queries are first parsed using an extended version of the corresponding grammar, and the resulting `AST` is
transformed into a set of tree-sitter queries
in `builder.rs`. 
The actual query matching is implemented in `query.rs`, which is a relatively small wrapper around tree-sitter's query engine to add weggli specific features. 


## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for details.

## License

Apache 2.0; see [`LICENSE`](LICENSE) for details.

## Disclaimer

This project is not an official Google project. It is not supported by
Google and Google specifically disclaims all warranties as to its quality,
merchantability, or fitness for a particular purpose.


