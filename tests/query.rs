/*
 Copyright 2021 Google LLC

 Licensed under the Apache License, Version 2.0 (the "License");
 you may not use this file except in compliance with the License.
 You may obtain a copy of the License at

      https://www.apache.org/licenses/LICENSE-2.0

 Unless required by applicable law or agreed to in writing, software
 distributed under the License is distributed on an "AS IS" BASIS,
 WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 See the License for the specific language governing permissions and
 limitations under the License.
 */

use weggli::builder::build_query_tree;
use simplelog::*;

fn parse_and_match_helper(needle: &str, source: &str, cpp: bool) -> usize {
    let _ = SimpleLogger::init(LevelFilter::Info, Config::default());
    log::set_max_level(log::LevelFilter::Debug);
    let tree = weggli::parse(needle, cpp);
    println!("{}", tree.root_node().to_sexp());

    let source_tree = weggli::parse(source, cpp);

    println!("{}", source_tree.root_node().to_sexp());

    let mut c = tree.walk();
    let qt = build_query_tree(needle, &mut c,  cpp);

    let matches = qt.matches(source_tree.root_node(), source);
    for m in &matches {
        println!("{}", m.display(source, 0, 0));
    }
    matches.len()
}

fn parse_and_match_cpp(needle: &str, source: &str) -> usize {
    parse_and_match_helper(needle, source, true)
}

fn parse_and_match(needle: &str, source: &str) -> usize {
    parse_and_match_helper(needle, source, false)
}

#[test]
fn simple() {
    assert_eq!(
        parse_and_match("{int $a = _+foo+$a;}", "void foo() {int bar=10+foo+bar;}"),
        1
    );
}

#[test]
fn while_simple() {
    let needle = "{while(_) {$i=10; bar=10;}}";
    let source = "void foo() {while(foo<10) {foo=10; x=1; bar=10;}}";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 1);
}

#[test]
fn type_simple() {
    let needle = "{$t $a = 3;}";
    let source = "void foo() {int foo = 3; uint16_t foo = 3; unsigned long foo = 3;}";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 3);
}

#[test]
fn type_variable() {
    let needle = "{$t $a = 3; $t foo = $a;}";
    let source =
        "void foo(){int bar = 3; int foo = bar; unsigned short foo = bar; char *ptr =foo;}";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 1);
}

#[test]
fn init_decl() {
    let needle = "{$foo = 100;}";
    let source = "void bar() {int foo = 100; char *foo =100;}";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 2);
}

#[test]
fn _loop() {
    let needle = "{$x $array[_]; for (int $i=0; _; _) {$array[$i]=_;}}";
    let source = "void bar() {char a[512]; for (int x=0;x<10;x++) {a[x]=0;}}";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 1);
}

#[test]
fn subquery() {
    let needle = "{int $something = _($i+1);}";
    let source = "void x(){int foo = 100*17-f(bar+1);}";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 1);
}

#[test]
fn subquery_vars() {
    let needle = "{int $i = _($i+$i);}";
    let source = "char b(){int foo = 100*17-f(bar+bar);}";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 0);
}

#[test]
fn subquery_nested() {
    let needle = "{int $i = _($i+_($i)); _($i)=10;}";
    let source = "void b(){int bar = 100*17-f(bar+x(bar)); bar = 10;}";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 1);
}

#[test]
fn wildcard() {
    let needle = "{for (_; _; $i++){$x[$i]=_;}}";
    let source = "void bar() {for(int i=0; i<10; i++) {buf[i]=10;}}";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 1);
}

#[test]
fn identifier_complex() {
    let source = r#"
        void foo() {
            abc = 1;
            def = 1;

            if (def > 1) {
                def = 10;
            }
            
        }"#;

    let needle = "{$x = 1; if ($x >1) {def = 10;}}";
    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 1);
}

#[test]
fn _type() {
    let source = r#"
        void foo() {
            int x = 10;
            unsigned char t = 0;
        }"#;

    let needle = "{unsigned char $x = _;}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 1);

    let needle = "{int $x = _;}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 1);
}

#[test]
fn exprstmt() {
    let source = r#"
        void foo() {
            a = func(bar);
        }"#;

    let needle = "{func(bar);}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 1);
}

#[test]
fn identifiers() {
    let needle = "{int x = func(bar); xonk(foo);}";
    let tree = weggli::parse(needle, false);

    let mut c = tree.walk();
    let qt = build_query_tree(needle, &mut c,  false);

    let identifiers = qt.identifiers();

    assert_eq!(identifiers, ["int", "x", "func", "bar", "xonk", "foo"]);
}

#[test]
fn compound() {
    let source = r#"
        void foo() {
            int x=10;
            if (x>10) {
                x=10;
            }
        }"#;

    let needle = "{$x=10; if($x>10) {$x=10;}}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 1);
}

#[test]
fn multiple_subqueries() {
    let source = r"
        int uvc_xu_ctrl_query(struct uvc_video_chain *chain,
            struct uvc_xu_control_query *xqry)
        {
            struct uvc_entity *entity;
            struct uvc_control *ctrl;
            unsigned int i;
            bool found;
            u32 reqflags;
            u16 size;
            u8 *data = NULL;
            int ret;
        
            /* Find the extension unit. */
            found = false;
            a=b;
        
            /* Find the control and perform delayed initialization if needed. */
            found = false;
            for (i = 0; i < entity->ncontrols; ++i) {
                ctrl = &entity->controls[i];
                if (ctrl->index == xqry->selector - 1) {
                    found = true;
                    break;
                }
            }
        
            if (!found) {
                return -ENOENT;
            }
        
            if (mutex_lock_interruptible(&chain->ctrl_mutex))
                return -ERESTARTSYS;
        
            ret = uvc_ctrl_init_xu_ctrl(chain->dev, ctrl);
            if (ret < 0) {
                ret = -ENOENT;
                goto done;
            }
        
            /* Validate the required buffer size and flags for the request */
            reqflags = 0;
            size = ctrl->info.size;

            data = kmalloc(size, GFP_KERNEL);
        }";

    let needle = "{u16 $size; $size = _; kmalloc($size);}";

    let matches = parse_and_match(needle, source);
    assert_eq!(1, matches);
}

#[test]
fn all_subqueries_must_match() {
    let source = r"
        static int ms_scsi_read(struct us_data *us, struct scsi_cmnd *srb)
        {
            u16 blen = ((cdb[7] << 8) & 0xff00) | ((cdb[8] << 0) & 0x00ff);
            u32 blenByte = blen * 0x200;

            if (bn > info->bl_num)
                return USB_STOR_TRANSPORT_ERROR;

            if (info->MS_Status.IsMSPro) {
            } else {
                void *buf;
                int offset = 0;
                u16 phyblk, logblk;
                u8 PageNum;
                u16 len;
                u32 blkno;

                buf = kmalloc(blenByte, GFP_KERNEL);
            }
            return result;  }";

    let needle = "{u16 $size; $size=_+_; kmalloc($size);}";

    let matches = parse_and_match(needle, source);
    assert_eq!(0, matches);
}

#[test]
fn field_expr() {
    let source = r#"
        void foo() {
            if (a->x > 10) {
                a->x = 10;
            }
        }"#;

    let needle = "{if($x>10) {$x=10;}}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 1);

    let source = r#"
        void foo() {
            if (a.x > 10) {
                a.x = 10;
            }
        }"#;

    let needle = "{if($x>10) {$x=10;}}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 1);
}

#[test]
fn casts() {
    let source = r#"
        int* foo() {
           bla *x = (bla *) malloc(10); 
        }"#;

    let needle = "{$x = malloc(_);}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 1);
}

#[test]
fn regression_1_nocaptures() {
    let source = r#"
        void foo() {
            if (a->x > 10) {
                a->x = 10;
            }
        }"#;

    let needle = "{;}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 1);
}

#[test]
fn regression_2_funcdefptr() {
    let source = r#"
    void *malloc() {
        return NULL;
    }"#;

    let needle = "_ $func() {return NULL;}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 1);
}

#[test]
fn simple_cpp() {
    let source = r#"
    #include <iostream>
    int main() {
    std::cout << "Hello World!";
    return 0;
    }"#;

    let needle = "_ $func() {std::cout << _;}";
    let matches = parse_and_match_cpp(needle, source);
    assert_eq!(matches, 1);
}

#[test]
fn class_cpp() {
    let source = r#"
    class Foo {
    void foo() {
        x = 10;
    }};"#;

    let needle = "_ $func() {$x=_;}";
    let matches = parse_and_match_cpp(needle, source);
    assert_eq!(matches, 1);
}

#[test]
fn negative_query() {
    let source = r#"
    void f() {
        char buf[10];
        if (i<10) {
        memcpy(buf,src, i);
        }
    }
    "#;

    let needle = "{char $b[_]; memcpy($b, _, $i);}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 1);

    let needle = "{char $b[_]; not: memcpy($b, _, $i);}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 0);

    let needle = "{char $b[_]; not: $i < _; memcpy($b, _, $i);}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 0);

    let needle = "{char $b[_]; not: $i > _; memcpy($b, _, $i);}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 1);

    let needle = "{char $b[_]; memcpy($b, _, $i); NOT: $i < 10;}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 1);
}

#[test]
fn negative_complex() {
    let source = r#"
    void f() {
        void *ptr;
        void *x;
        ptr = NULL;
        g(&ptr);
        g(&x);
        x = NULL;
        }
    }
    "#;

    let needle = "{_ *$p; $func(&$p);}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 2);

    let needle = "{_ *$p; NOT: $p = _; $func(&$p);}";
    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 1);
}

#[test]
fn negative_if() {
    let source = r#"
    void f() {
        if (size > 1) return;
        memcpy(&value,buffer,size);
        size = 1;
    }
    "#;

    let needle = "_ $func(){if ($size > 1 ) return;
        memcpy(_,_, $size);
        $size = 1;}";
    let matches = parse_and_match_cpp(needle, source);
    assert_eq!(matches, 1);

    let needle = "_ $func(){NOT: if ($size > 1 ) return;
        memcpy(_,_, $size);
        $size = 1;}";
    let matches = parse_and_match_cpp(needle, source);
    assert_eq!(matches, 0);
}

#[test]
fn cpp_namespace() {
    let source = r#"
    void Test::abcdef::hello() {
        int size = 1;
        return;
    }
    "#;

    let needle = "_ $fn(){
        size = 1;
        }";
    let matches = parse_and_match_cpp(needle, source);
    assert_eq!(matches, 1);

    let needle = "_ _::$fn(){
        size = 1;
        }";
    let matches = parse_and_match_cpp(needle, source);
    assert_eq!(matches, 1);
}

#[test]
fn var_regression() {
    let source = r#"
    void foo() {
        a::b;
        w.func(x,x);
      }
    "#;

    let needle = "{
        $y::$z;
        _.func($x,$x);
    }";
    let matches = parse_and_match_cpp(needle, source);
    assert_eq!(matches, 1);
}

#[test]
fn not_decl() {
    let source = r#"
    void foo() {
        int x;
        bar(x);
      }
    "#;

    let needle = "{
        not: {int x;}
        bar(x);
    }";

    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 0);
}

#[test]
fn func_calls() {
    let source = r#"
    void foo() {
        a->bar(x);
        b.bar(x);
        b->bar->c(x);
      }
    "#;

    let needle = "{
        bar(x);
    }";

    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 2);

    let source = r#"
    void foo() {
        std::memcpy(a,b,c);
        memcpy(a,b,c);
        a::b::c::d::e::memcpy(a,b,c);
        a->func(a,b,c);
      }
    "#;

    let needle = "{
        memcpy(_);
    }";

    let matches = parse_and_match_cpp(needle, source);
    assert_eq!(matches, 3);

    let needle = "{
        $func(_);
    }";

    let matches = parse_and_match_cpp(needle, source);
    assert_eq!(matches, 4);
}

#[test]
fn tertiary() {
    let source = r#"
    void foo() {
    int16_t next = (d->flags & flag)
                       ? d->next
                       : EndOfChain;
    }"#;

    let needle = "{
       _ = _(_ ? d->next : _);
    }";

    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 1);

    let needle = "{
       $t _ = _ ? d->next : _;
    }";

    let matches = parse_and_match(needle, source);
    assert_eq!(matches, 1);
}

#[test]
fn not_regression() {
    // https://github.com/googleprojectzero/weggli/issues/2
    let needle = "{free($handle); not: $handle= NULL;}";
    let source = r"
    void func()
    {
        free(data); //this should not match
        data = NULL ; 
        
        free(handle); //this should match
    }";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 1);
}


#[test]
fn allow_empty_blocks() {
    let needle = "{if ($x){}}";
    let source = r"
    void func(){
    if (foo) {
        a = 1;
        b = 2;
        c = 3;
    }}";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 1);

}

#[test]
fn filter_identical_matches() {
    // https://github.com/googleprojectzero/weggli/issues/3
    let needle = "{if ($x){_;}}";
    let source = r"
    void func(){
    if (foo) {
        a = 1;
        b = 2;
        c = 3;
    }}";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 1);
}

#[test]
fn test_commutative() {
    let needle = "{if ($x + size > 0){}}";
    let source = r"
    void func(){
    if (size + offset > 0) {
        func2();
    }}";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 1);

    let needle = "{while (_(C_INUSE & _($psize))){k == $psize;}}";
    let source = r"
    static int alloc_rev(struct chunk *c)
    {
	int i;
	size_t k;
	while (!((k=c->psize) & C_INUSE)) {
		i = bin_index(k);
		lock_bin(i);
		if (c->psize == k) {
			unbin(PREV_CHUNK(c), i);
			unlock_bin(i);
			return 1;
		}
		unlock_bin(i);
	}
	return 0;
    }
    ";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 1);

    let needle = "{if ($x - size > 0){}}";
    let source = r"
    void func(){
    if (size - offset > 0) {
        func2();
    }}";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 0);

    let needle = "{if ($x / size > 0){}}";
    let source = r"
    void func(){
    if (size / offset > 0) {
        func2();
    }}";

    let matches = parse_and_match(needle, source);

    assert_eq!(matches, 0);
}

#[test]
fn test_comparisons() {
    let needle = "{if ($x + size > dst_size){}}";
    let source = r"
    void func(){
    if (dst_size < size + $x) {
        func2();
    }}";

    let matches = parse_and_match_cpp(needle, source);

    assert_eq!(matches, 1);

    let needle = "{while ($x <= max) {$x++;}}";
    let source = r"
    void func(){
        while (max >= count) {count++;}
    }";

    let matches = parse_and_match_cpp(needle, source);

    assert_eq!(matches, 1);
}

#[test]
fn test_numbers() {
    let needle = "{$x = 10;}";
    let source = r"
    void func(){
        a = 10; // match
        b = 0xa; // match
        c = 10u; // match
        d = 012; // match
        f = 0x100; // no match
        g = 0x10; // no match
        h = 010; // no match
        i = 3.14 // no match 
    }}";

    let matches = parse_and_match_cpp(needle, source);

    assert_eq!(matches, 4);
}