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

use assert_cmd::prelude::*; // Add methods on commands
use predicates::prelude::*; // Used for writing assertions
use std::process::Command; // Run programs

#[test]
fn invalid_pattern() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("weggli")?;

    cmd.arg("{foo").arg("directory");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Query parsing failed"));

    Ok(())
}

#[test]
fn flags_empty_results() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("weggli")?;

    cmd.arg("-u")
        .arg("--extensions=a,b,c")
        .arg("foo();")
        .arg("directory");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("No files to parse."));
    Ok(())
}

#[test]
fn exclude_include() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("weggli")?;

    cmd.arg("-u")
        .arg("--exclude=[e]xamples/")
        .arg("foo();")
        .arg("./third_party/examples/");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("No files to parse."));

    let mut cmd = Command::cargo_bin("weggli")?;
    cmd.arg("-u")
        .arg("--include=doesnotexist")
        .arg("foo();")
        .arg("./third_party/examples/");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("No files to parse."));

    let mut cmd = Command::cargo_bin("weggli")?;
    cmd.arg("-u")
        .arg("--include=cluster.c$")
        .arg("foo();")
        .arg("./third_party/examples/");
    cmd.assert()
        .success()
        .stderr(predicate::str::is_empty())
        .stdout(predicate::str::is_empty());

    let mut cmd = Command::cargo_bin("weggli")?;
    cmd.arg("--include=doesnotexist")
        .arg("--include=cluster.c$")
        .arg("foo();")
        .arg("./third_party/examples/");
    cmd.assert()
        .success()
        .stderr(predicate::str::is_empty())
        .stdout(predicate::str::is_empty());

    let mut cmd = Command::cargo_bin("weggli")?;
    cmd.arg("--include=invalidregex((")
        .arg("foo();")
        .arg("./third_party/examples/");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("regex parse error"));

    Ok(())
}

#[test]
fn successfull_run() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("weggli")?;

    cmd.arg("-Rfun=memcpy")
        .arg("$fun(_,_,sizeof(_));")
        .arg("./third_party/examples/cluster.c");
    cmd.assert().success().stdout(predicate::str::contains(
        "void clusterBuildMessageHdr(clusterMsg *hdr, int type) {",
    ));

    Ok(())
}

#[test]
fn multi_pattern() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("weggli")?;

    cmd.arg("$func(_,hdr->sender)")
        .arg("-p _ $func($ptr *$src){memcpy (_,$src,_);}")
        .arg("./third_party/examples/cluster.c");
    cmd.assert().success().stdout(
        predicate::str::contains("int clusterProcessPacket(clusterLink *link) {")
            .and(predicate::str::contains("memcmp(link->node->name,hdr->sender").not()),
    );

    Ok(())
}

#[test]
fn multi_pattern_empty() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("weggli")?;

    cmd.arg("$src=1;")
        .arg("-p $func(_,hdr->sender);")
        .arg("-p _ $func($ptr *$src){memcpy (_,$src,_);}")
        .arg("./third_party/examples/cluster.c");
    cmd.assert().success().stdout(predicate::str::is_empty());

    Ok(())
}

#[test]
fn test_unique() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("weggli")?;

    cmd.arg("{$a = sdssplitargs(_); strcasecmp($b[0],_); }")
        .arg("./third_party/examples/cluster.c");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("clusterLoadConfig"));

    let mut cmd = Command::cargo_bin("weggli")?;
    cmd.arg("{$a = sdssplitargs(_); strcasecmp($b[0],_); }")
        .arg("./third_party/examples/cluster.c")
        .arg("--unique");
    cmd.assert().success().stdout(predicate::str::is_empty());

    Ok(())
}

#[test]
fn invalid_utf8() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("weggli")?;

    cmd.arg("memcpy(_,_,_);")
        .arg("./third_party/examples/invalid-utf8.c");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("memcpy"));

    Ok(())
}

#[test]
fn regex_constraint() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("weggli")?;

    cmd.arg("char $buf[10];")
        .arg("./third_party/examples/invalid-utf8.c")
        .arg("-Rbuf=buf");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("char buf[10]"));

    let mut cmd = Command::cargo_bin("weggli")?;

    cmd.arg("char $buf[10];")
        .arg("./third_party/examples/invalid-utf8.c")
        .arg("-Rbuf=foo");
    cmd.assert().success().stdout(predicate::str::is_empty());

    let mut cmd = Command::cargo_bin("weggli")?;

    cmd.arg("char $buf[10];")
        .arg("./third_party/examples/invalid-utf8.c")
        .arg("-Rruf=foo");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("is not a valid query variable"));

    let mut cmd = Command::cargo_bin("weggli")?;

    cmd.arg("char $buf[10];")
        .arg("./third_party/examples/invalid-utf8.c")
        .arg("-Rbuf!=woof");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("char buf[10]"));

    let mut cmd = Command::cargo_bin("weggli")?;

    cmd.arg("{char buf[10]; not: memcpy($buf, _, _);}")
        .arg("./third_party/examples/invalid-utf8.c")
        .arg("-Rbuf=woof");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("char buf[10]"));

    let mut cmd = Command::cargo_bin("weggli")?;

    cmd.arg("{char buf[10]; not: memcpy($buf, _, _);}")
        .arg("./third_party/examples/invalid-utf8.c")
        .arg("-Rbuf=buf");
    cmd.assert().success().stdout(predicate::str::is_empty());

    let mut cmd = Command::cargo_bin("weggli")?;

    cmd.arg("{char buf[10]; not: memcpy($buf, _, _);}")
        .arg("./third_party/examples/invalid-utf8.c")
        .arg("-Rbuf!=buf");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("char buf[10]"));

    Ok(())
}
