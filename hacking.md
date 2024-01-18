# Hacking weggli

## Goal of the document

This document's goal is to give a high level overview of a *weggli* - how it works, what are the basic building blocks and how to navigate the source code. We hope that it will help future developers to quickly comprehend main concepts and allow them to either make significant changes in the code or implement new features.


## A tree-sitter parser

The most important external library used in *weggli* is Tree-sitter - a parser generator that combines the ability to transform a source code into an AST (*Abstract Syntax Tree*) as well as running complex queries against such AST. Knowledge of how to use this library is essential if you want to add new rules or support for new programming languages.

As it was already mentioned - two most important elements of this library is a parser that can turn a source code into an AST and a query language that could find certain patterns inside such AST.

To discover their inner working we can check the example below.

```c
{
  int x = 10;
  void *y = malloc(x);
}
```

If we transform those two C instructions into an AST we would end up with a following one:

```
translation_unit [0, 0] - [4, 0]
  compound_statement [0, 0] - [3, 1]
    declaration [1, 2] - [1, 13]
      type: primitive_type [1, 2] - [1, 5]
      declarator: init_declarator [1, 6] - [1, 12]
        declarator: identifier [1, 6] - [1, 7]
        value: number_literal [1, 10] - [1, 12]
    declaration [2, 2] - [2, 22]
      type: primitive_type [2, 2] - [2, 6]
      declarator: init_declarator [2, 7] - [2, 21]
        declarator: pointer_declarator [2, 7] - [2, 9]
          declarator: identifier [2, 8] - [2, 9]
        value: call_expression [2, 12] - [2, 21]
          function: identifier [2, 12] - [2, 18]
          arguments: argument_list [2, 18] - [2, 21]
            identifier [2, 19] - [2, 20]
```

Now, if we are interested in finding certain patterns in the code we can write a query that looks like this.

```scheme
(
	(declaration (init_declarator value: (call_expression (identifier) @1)))
    (#eq? @1 "malloc")
)
```

Applying this query to aforementioned AST will result in finding a `malloc()` call.

## Life of a query

**Parameters**

The life of a *weggli* query begins when the user provides a set of parameters to the executable. The most important ones are *Pattern* and *Path*. Pattern is an expression in a weggli query language that closely resembles C/C++ with a small number of extra features. *Path* is just a file or directory that we are going to process looking for our pattern. Parameter extraction is happening in `cli::parse_argument()` and it stores all the results in `Args` structure. Besides the already mentioned parameters we are also capturing a lot of supplementary ones. You can find more about them by reading the `src/cli.rs` file.

**Pattern normalization**

User-provided pattern is first sent to a `parse_search_pattern()` function where it is normalized (fixing missing semicolon or lack of curly braces) and validated. After normalization we end up with a tree-sitter AST of our pattern represented by a `Tree` type. Validation is happening inside `validate_query()` function and the main objective is to verify if it has no syntax errors and if it is *rooted* correctly. In the absence of error function returns a `TreeCursor` that points to the root node of the AST of a user pattern.

> In weggli a correctly rooted expression means that it has a single root of one of the expected types. So, in normal terms - if this is a single compound statement, function definition or valid `struc`, `enum`, `union` or `class`.

**Building a weggli query**

The real heavy lifting starts when we pass the cursor to a `builder::build_query_tree()` - function responsible for turning our AST into a tree-sitter search query. This query will reside in `QueryTree` - along variables, captures and negations. The important part is that a single user pattern will usually result in a tree of sub-queries. Main reason is tree-sitter query language inability to search iteratively. A typical example would be a nested function calls like `int x = func_1(func_2(buf))` - searching for a `func_1($buf)` would miss the nested calls. 

When the `QueryTree` is ready we can put it into a `WorkItem` together with all the defined *identifiers* like function names, variables and types.

> Captures are simply a variables like `$var` that we have defined in our pattern. 
>
> Negation is simply a negative query that later on will be used to filter out results that match this particular branch.

**Dispatch query to workers**

When our pattern is finally transformed into a tree of tree-sitter queries and a set of files to be scanned is locked we are ready to start our workers.

We begin with `parse_files_worker()` as our first line of workers. What happens here is that we have a *pool* of threads that process the files we've defined as our target. Processing actually is happening in two steps - in the first step we simply check if the file in a *raw* form contains any of the identifiers we are interested in. If this is not the case then this file is skipped, otherwise it is transformed into an AST using again a tree-sitter parser and sent to the second line of workers via an established mpsc channel.

The `execute_queries_worker()` function starts second line workers. Their main task is to recivce an AST of the target files and apply a set of queries from the `WorkItem` to them.

The whole process of running a query against given AST is happening in multiple stages as well and the starting point is `QueryTree.match_internal()` and some more stages that follows usually involve filtering out duplicates and enforcing some limit.

In case we were running multiple queries there is also a third line worker spawned by `multi_query_worker()` function. Main job of this worker is to capture all independent results to filter them looking if variable assignments are valid for all the queries. Regardless if we have gone through the last line of workers or not we end up with an array of `QueryResult` objects that represent all our findings.

**Displaying results**

Each of the `QueryResult` objects has a `display()` method that is responsible for printing the results. It always prints out the found node and surrounding lines of code to the console - at least for now. The function also tries to merge multiple different findings into one where applicable (for example if there are two findings in the same function).

## Implementation details

### Building a tree sitter query

<to be added later>

### Running queries against AST

<to be added later>

### Gathering results

<to be added later>

