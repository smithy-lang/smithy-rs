/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.TreeMap;
import java.util.stream.Collectors;
import java.util.stream.Stream;
import software.amazon.smithy.model.Model;
import software.amazon.smithy.model.node.ArrayNode;
import software.amazon.smithy.model.node.Node;
import software.amazon.smithy.model.node.ObjectNode;
import software.amazon.smithy.model.shapes.ServiceShape;
import software.amazon.smithy.rulesengine.aws.s3.S3TreeRewriter;
import software.amazon.smithy.rulesengine.language.EndpointRuleSet;
import software.amazon.smithy.rulesengine.language.evaluation.RuleEvaluator;
import software.amazon.smithy.rulesengine.language.evaluation.value.Value;
import software.amazon.smithy.rulesengine.language.syntax.Identifier;
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter;
import software.amazon.smithy.rulesengine.language.syntax.parameters.ParameterType;
import software.amazon.smithy.rulesengine.logic.bdd.CostOptimization;
import software.amazon.smithy.rulesengine.logic.bdd.NodeReversal;
import software.amazon.smithy.rulesengine.logic.bdd.SiftingOptimization;
import software.amazon.smithy.rulesengine.logic.cfg.Cfg;
import software.amazon.smithy.rulesengine.traits.EndpointBddTrait;
import software.amazon.smithy.rulesengine.traits.EndpointRuleSetTrait;
import software.amazon.smithy.rulesengine.transforms.CompileBdd;

/**
 * Records every stage of Smithy Java's {@code CompileBdd} for each rule set in the corpus.
 *
 * <p>Usage: {@code java -cp CLASSPATH Generate.java FIXTURES_DIR AWS_MODELS_DIR [--patched]}; run
 * through {@code generate.sh}. See README.md.
 *
 * <p>With {@code --patched}, {@code java-fixes.patch} must be on the classpath ahead of the jars.
 * That run checks that every existing fixture is unchanged, then records the random seeds that
 * unpatched Java failed on.
 */
public final class Generate {
    private static final String PINNED = "1.73.0";
    private static final String S3 = "com.amazonaws.s3#AmazonS3";
    private static final String RULE_SET = "smithy.rules#endpointRuleSet";
    private static final String BDD = "smithy.rules#endpointBdd";
    private static final int RANDOM_CASES = 40;
    private static final String FIXES = "CompileBdd with java-fixes.patch";
    private static boolean patched;

    public static void main(String[] args) throws Exception {
        patched = args.length > 2 && args[2].equals("--patched");
        Class<?> coalesce = Class.forName("software.amazon.smithy.rulesengine.logic.cfg.CoalesceTransform");
        boolean loadedPatch = !coalesce.getProtectionDomain().getCodeSource().getLocation().getPath().endsWith(".jar");
        if (patched != loadedPatch) {
            throw new IllegalStateException(patched ? "java-fixes.patch is not on the classpath"
                    : "java-fixes.patch is on the classpath without --patched");
        }
        String version = EndpointRuleSet.class.getPackage().getImplementationVersion();
        if (version != null && !version.equals(PINNED)) {
            throw new IllegalStateException("expected smithy-rules-engine " + PINNED + ", found " + version);
        }
        Path expected = Paths.get(args[0]).resolve("expected");
        Path awsModels = Paths.get(args[1]);
        Files.createDirectories(expected);
        ObjectNode previous = patched
                ? Node.parse(Files.readString(expected.resolve("manifest.json"))).expectObjectNode()
                : null;
        try (Stream<Path> old = patched ? Stream.empty() : Files.list(expected)) {
            for (Path path : old.collect(Collectors.toList())) {
                Files.delete(path);
            }
        }

        Map<String, String> manifest = new TreeMap<>();
        // The AWS SDK models, whose checked-in BDDs Java must reproduce.
        List<Path> models;
        try (Stream<Path> paths = Files.list(awsModels)) {
            models = paths.filter(p -> p.toString().endsWith(".json")).sorted().collect(Collectors.toList());
        }
        for (Path path : models) {
            String text = Files.readString(path);
            if (!text.contains(RULE_SET)) {
                continue;
            }
            String stem = path.getFileName().toString().replaceFirst("\\.json$", "");
            ObjectNode shapes = Node.parse(text).expectObjectNode().expectObjectMember("shapes");
            for (Map.Entry<String, Node> shape : shapes.getStringMap().entrySet()) {
                ObjectNode traits = shape.getValue().expectObjectNode().getObjectMember("traits").orElse(null);
                if (traits == null || !traits.containsMember(RULE_SET)) {
                    continue;
                }
                String source = "aws/sdk/aws-models/" + path.getFileName() + "#" + shape.getKey();
                EndpointRuleSet rules = EndpointRuleSet.fromNode(traits.expectMember(RULE_SET));
                Node checkedIn = traits.getMember(BDD).orElse(null);
                boolean isS3 = shape.getKey().equals(S3);
                // S3's checked-in BDD comes from CompileBddForAws, which rewrites the rule set first.
                // The input is read from the model, so it is not repeated in the fixture.
                write(expected, manifest, "aws-" + stem, source, "CompileBdd", rules, isS3 ? null : checkedIn, false);
                if (isS3) {
                    // The rewrite introduces negative getAttr indexes but keeps version 1.0, which
                    // RulesEngineVersionValidator would reject. Record it as 1.1: the compiled
                    // BDD is 1.1 either way, and is still checked against the checked-in trait.
                    ObjectNode rewritten = S3TreeRewriter.transform(rules).toNode().expectObjectNode();
                    String from = rewritten.expectStringMember("version").getValue();
                    EndpointRuleSet raised = EndpointRuleSet.fromNode(rewritten.withMember("version", "1.1"));
                    if (!CompileBdd.compileBdd(raised).toNode()
                            .equals(CompileBdd.compileBdd(EndpointRuleSet.fromNode(rewritten)).toNode())) {
                        throw new IllegalStateException("raising the S3 rewrite's version changes compilation");
                    }
                    write(expected, manifest, "aws-" + stem + "-rewritten", source,
                            "S3TreeRewriter+CompileBdd (version " + from + " recorded as 1.1)", raised, checkedIn, true);
                }
            }
        }

        // The rule sets shipped in smithy-rules-engine-tests, which cover newer functions.
        Model model = Model.assembler(Generate.class.getClassLoader())
                .discoverModels(Generate.class.getClassLoader())
                .assemble()
                .unwrap();
        for (ServiceShape service : model.getServiceShapes()) {
            if (!service.hasTrait(EndpointRuleSetTrait.ID)
                    || !service.getId().getNamespace().equals("smithy.rules.tests")) {
                continue;
            }
            EndpointRuleSet rules = service.expectTrait(EndpointRuleSetTrait.class).getEndpointRuleSet();
            write(expected, manifest, "tests-" + service.getId().getName(),
                    "smithy-rules-engine-tests:" + PINNED + "#" + service.getId(), "CompileBdd", rules, null, true);
        }

        // Seeded random rule sets, covering shapes the corpus does not: reused assignment names
        // across siblings (SSA renaming), deep nesting, negations, and many small conditions.
        // Java's compiler fails on some of these valid inputs (see README.md). Those seeds are listed
        // in the manifest with Java's error; seeds continue until RANDOM_CASES compile.
        // The patched run records those too, after checking that the fixes change no other fixture.
        Map<String, String> javaFailures = new TreeMap<>();
        if (patched) {
            previous.expectObjectMember("javaFailures").getStringMap()
                    .forEach((k, v) -> javaFailures.put(k, v.expectStringNode().getValue()));
        }
        int compiled = 0;
        for (int seed = 1; compiled < RANDOM_CASES; seed++) {
            if (seed > 10 * RANDOM_CASES) {
                throw new IllegalStateException("too many random cases fail to compile");
            }
            String name = String.format("random-%03d", seed);
            Node node = new RandomRuleSet(new Random(seed)).build();
            // Skip rule sets whose trees too rarely fall through to a result to be checked.
            if (samples(name, EndpointRuleSet.fromNode(node)).size() < 30) {
                System.out.println(name + ": too few samples evaluate; skipped");
                continue;
            }
            if (javaFailures.containsKey(name)) {
                if (patched) {
                    write(expected, manifest, name, "Generate.java RandomRuleSet seed " + seed, FIXES,
                            EndpointRuleSet.fromNode(node), null, true);
                }
                continue;
            }
            try {
                write(expected, manifest, name, "Generate.java RandomRuleSet seed " + seed, "CompileBdd",
                        EndpointRuleSet.fromNode(node), null, true);
                compiled++;
            } catch (Exception e) {
                if (patched) {
                    throw e;
                }
                Throwable cause = e;
                while (cause.getCause() != null) {
                    cause = cause.getCause();
                }
                javaFailures.put(name, cause.getMessage());
                System.out.println(name + ": Java failed: " + cause.getMessage());
            }
        }

        if (patched && !manifest.keySet().containsAll(
                previous.expectObjectMember("fixtures").getStringMap().keySet())) {
            throw new IllegalStateException("the patched run is missing fixtures");
        }
        ObjectNode.Builder index = ObjectNode.builder()
                .withMember("smithyRulesEngine", PINNED)
                .withMember("java", System.getProperty("java.version"));
        ObjectNode.Builder fixtures = ObjectNode.builder();
        manifest.forEach(fixtures::withMember);
        index.withMember("fixtures", fixtures.build());
        ObjectNode.Builder failures = ObjectNode.builder();
        javaFailures.forEach(failures::withMember);
        index.withMember("javaFailures", failures.build());
        Files.writeString(expected.resolve("manifest.json"), Node.prettyPrintJson(index.build()) + "\n");
        System.out.println("wrote " + manifest.size() + " fixtures");
    }

    private static void write(
            Path expected,
            Map<String, String> manifest,
            String name,
            String source,
            String pipeline,
            EndpointRuleSet rules,
            Node checkedIn,
            boolean includeRuleSet
    ) throws Exception {
        ObjectNode first = stages(rules);
        // The pipeline must be deterministic for these fixtures to be a contract.
        if (!first.equals(stages(rules))) {
            throw new IllegalStateException(name + ": compilation is not deterministic");
        }
        // The stages must reproduce the real entry point...
        if (!first.expectMember("compiled").equals(CompileBdd.compileBdd(rules).toNode())) {
            throw new IllegalStateException(name + ": staged compilation differs from CompileBdd.compileBdd");
        }
        // ...and the checked-in trait it is compared against.
        if (checkedIn != null && !first.expectMember("compiled").equals(checkedIn)) {
            throw new IllegalStateException(name + ": compiled BDD differs from the checked-in endpointBdd");
        }
        if (name.startsWith("random-")) {
            checkEquivalent(name, rules, first.expectMember("compiled"));
        }
        ObjectNode fixture = ObjectNode.builder()
                .withMember("source", source)
                .withMember("pipeline", pipeline)
                .withMember("matchesCheckedIn", checkedIn != null)
                .merge(compact(name, first, includeRuleSet))
                .build();
        Path file = expected.resolve(name + ".json");
        String text = Node.printJson(fixture) + "\n";
        if (patched && !pipeline.equals(FIXES)) {
            if (!Files.readString(file).equals(text)) {
                throw new IllegalStateException(name + ": java-fixes.patch changes this fixture");
            }
        } else {
            Files.writeString(file, text);
        }
        manifest.put(name, source);
        System.out.println(name + ": " + first.expectObjectMember("compiled").expectNumberMember("nodeCount").getValue()
                + " nodes");
    }

    /**
     * Evaluates a random rule set on seeded parameter values, returning each value and its result.
     * Random rule sets only use functions both of Java's evaluators support.
     *
     * <p>Random trees need not end with an unconditional rule. When none of a matched tree's rules
     * match, {@code RuleEvaluator} reports invalid rules rather than falling through, so those
     * samples are left out.
     */
    private static List<Map.Entry<Map<Identifier, Value>, String>> samples(String name, EndpointRuleSet rules) {
        Random random = new Random(name.hashCode());
        String[] pool = {"r", "x0", "x1", "x2", "ax0", "xx1", "x0x1", "abx2y", ""};
        List<Map.Entry<Map<Identifier, Value>, String>> out = new ArrayList<>();
        for (int i = 0; i < 300; i++) {
            Map<Identifier, Value> args = new HashMap<>();
            for (Parameter parameter : rules.getParameters()) {
                if (parameter.getType() == ParameterType.BOOLEAN) {
                    args.put(parameter.getName(), Value.booleanValue(random.nextBoolean()));
                } else if (parameter.isRequired() || random.nextInt(3) > 0) {
                    String value = pool[random.nextInt(pool.length)];
                    if (random.nextInt(4) == 0) {
                        StringBuilder text = new StringBuilder();
                        for (int c = random.nextInt(7); c > 0; c--) {
                            text.append("ax012".charAt(random.nextInt(5)));
                        }
                        value = text.toString();
                    }
                    args.put(parameter.getName(), Value.stringValue(value));
                }
            }
            try {
                out.add(Map.entry(args, RuleEvaluator.evaluate(rules, args).toString()));
            } catch (RuntimeException e) {
                if (!String.valueOf(e.getMessage()).startsWith("no rules inside of tree rule matched")) {
                    throw e;
                }
            }
        }
        return out;
    }

    /** Checks that the compiled BDD selects the same result as the rule set for every sample. */
    private static void checkEquivalent(String name, EndpointRuleSet rules, Node compiled) {
        EndpointBddTrait bdd = EndpointBddTrait.fromNode(compiled);
        for (Map.Entry<Map<Identifier, Value>, String> sample : samples(name, rules)) {
            String actual = RuleEvaluator.evaluate(bdd, sample.getKey()).toString();
            if (!sample.getValue().equals(actual)) {
                throw new IllegalStateException(name + ": the BDD selects " + actual + " instead of "
                        + sample.getValue() + " for " + sample.getKey());
            }
        }
    }

    /**
     * Shrinks the recorded stages without losing information.
     *
     * <p>Parameters, results, and version are equal in every BDD stage, so only {@code compiled}
     * keeps them. Intermediate stages record their conditions as indexes into
     * {@code cfgConditions}. {@code ssa} is omitted when it equals the input rule set.
     */
    private static ObjectNode compact(String name, ObjectNode stages, boolean includeRuleSet) {
        ObjectNode compiled = stages.expectObjectMember("compiled");
        List<Node> cfgConditions = stages.expectArrayMember("cfgConditions").getElements();
        ObjectNode.Builder out = ObjectNode.builder();
        if (includeRuleSet) {
            out.withMember("ruleSet", stages.expectMember("ruleSet"));
        }
        if (!stages.expectMember("ssa").equals(stages.expectMember("ruleSet"))) {
            out.withMember("ssa", stages.expectMember("ssa"));
        }
        out.withMember("cfgConditions", stages.expectMember("cfgConditions"));
        for (String stage : new String[] {"unoptimized", "sifted", "costOptimized", "reversed"}) {
            ObjectNode bdd = stages.expectObjectMember(stage);
            for (String shared : new String[] {"version", "parameters", "results"}) {
                if (!bdd.expectMember(shared).equals(compiled.expectMember(shared))) {
                    throw new IllegalStateException(name + ": " + stage + " `" + shared + "` differs from compiled");
                }
            }
            List<Node> order = new ArrayList<>();
            for (Node condition : bdd.expectArrayMember("conditions").getElements()) {
                int index = cfgConditions.indexOf(condition);
                if (index < 0) {
                    throw new IllegalStateException(name + ": " + stage + " has a condition not in the CFG");
                }
                order.add(Node.from(index));
            }
            out.withMember(stage, ObjectNode.builder()
                    .withMember("order", ArrayNode.fromNodes(order))
                    .withMember("root", bdd.expectMember("root"))
                    .withMember("nodeCount", bdd.expectMember("nodeCount"))
                    .withMember("nodes", bdd.expectMember("nodes"))
                    .build());
        }
        return out.withMember("compiled", compiled).build();
    }

    /** Runs CompileBdd.compileBdd one stage at a time, recording each stage's output. */
    private static ObjectNode stages(EndpointRuleSet rules) throws Exception {
        // CfgBuilder applies SsaTransform (which runs the coalescing and binding transforms first).
        Method ssa = Class.forName("software.amazon.smithy.rulesengine.logic.cfg.SsaTransform")
                .getDeclaredMethod("transform", EndpointRuleSet.class);
        ssa.setAccessible(true);
        EndpointRuleSet ssaRules = (EndpointRuleSet) ssa.invoke(null, rules);

        Cfg cfg = Cfg.from(rules);
        List<Node> cfgConditions = new ArrayList<>();
        for (var condition : cfg.getConditions()) {
            cfgConditions.add(condition.toNode());
        }
        EndpointBddTrait unoptimized = EndpointBddTrait.from(cfg);
        EndpointBddTrait sifted = SiftingOptimization.builder().cfg(cfg).build().apply(unoptimized);
        EndpointBddTrait costOptimized = CostOptimization.builder().cfg(cfg).build().apply(sifted);
        EndpointBddTrait reversed = new NodeReversal().apply(costOptimized);
        EndpointBddTrait compiled = reversed.removeUnreferencedConditions();
        return ObjectNode.builder()
                .withMember("ruleSet", rules.toNode())
                .withMember("ssa", ssaRules.toNode())
                .withMember("cfgConditions", ArrayNode.fromNodes(cfgConditions))
                .withMember("unoptimized", unoptimized.toNode())
                .withMember("sifted", sifted.toNode())
                .withMember("costOptimized", costOptimized.toNode())
                .withMember("reversed", reversed.toNode())
                .withMember("compiled", compiled.toNode())
                .build();
    }

    /**
     * Builds a well-typed, well-scoped rule set from a seed.
     *
     * <p>Every reference is a parameter or an assignment in scope, and assignment names come from
     * a small pool so that siblings reuse them with different expressions. Optional parameters
     * are only used as strings after an {@code isSet} in scope proves them set, as Java's type
     * checker requires.
     */
    private static final class RandomRuleSet {
        private static final String[] NAMES = {"a", "b", "c"};
        private final Random random;
        private final int optionals;
        private final int booleans;
        private int rules;

        RandomRuleSet(Random random) {
            this.random = random;
            this.optionals = 1 + random.nextInt(3);
            this.booleans = 1 + random.nextInt(2);
        }

        /** What is in scope: string values usable directly, and optionals not yet proven set. */
        private static final class Scope {
            final List<String> strings;
            final List<String> unproven;

            Scope(List<String> strings, List<String> unproven) {
                this.strings = new ArrayList<>(strings);
                this.unproven = new ArrayList<>(unproven);
            }

            Scope copy() {
                return new Scope(strings, unproven);
            }
        }

        Node build() {
            ObjectNode.Builder parameters = ObjectNode.builder();
            parameters.withMember("R", ObjectNode.builder().withMember("type", "string")
                    .withMember("required", true).withMember("default", "r")
                    .withMember("documentation", "d").build());
            List<String> unproven = new ArrayList<>();
            for (int i = 0; i < optionals; i++) {
                parameters.withMember("S" + i, ObjectNode.builder().withMember("type", "string")
                        .withMember("documentation", "d").build());
                unproven.add("S" + i);
            }
            for (int i = 0; i < booleans; i++) {
                parameters.withMember("B" + i, ObjectNode.builder().withMember("type", "boolean")
                        .withMember("required", true).withMember("default", false)
                        .withMember("documentation", "d").build());
            }
            List<Node> top = rules(0, new Scope(List.of("R"), unproven));
            top.add(error("no match", List.of()));
            return ObjectNode.builder().withMember("version", "1.1")
                    .withMember("parameters", parameters.build())
                    .withMember("rules", ArrayNode.fromNodes(top)).build();
        }

        private List<Node> rules(int depth, Scope scope) {
            List<Node> out = new ArrayList<>();
            int count = 1 + random.nextInt(depth == 0 ? 5 : 3);
            for (int i = 0; i < count && rules < 60; i++) {
                rules++;
                Scope local = scope.copy();
                List<Node> conditions = new ArrayList<>();
                int n = random.nextInt(4);
                for (int c = 0; c < n; c++) {
                    conditions.add(condition(local));
                }
                int kind = random.nextInt(depth >= 4 ? 2 : 4);
                if (kind >= 2) {
                    List<Node> children = rules(depth + 1, local);
                    if (children.isEmpty()) {
                        // The rule budget ran out; a tree needs at least one rule.
                        children.add(error("error " + rules, List.of()));
                    }
                    out.add(ObjectNode.builder().withMember("type", "tree")
                            .withMember("conditions", ArrayNode.fromNodes(conditions))
                            .withMember("rules", ArrayNode.fromNodes(children)).build());
                } else if (kind == 1) {
                    out.add(error("error " + rules + " {" + pick(local.strings) + "}", conditions));
                } else {
                    String url = "https://" + rules + ".{" + pick(local.strings) + "}.example.com";
                    out.add(ObjectNode.builder().withMember("type", "endpoint")
                            .withMember("conditions", ArrayNode.fromNodes(conditions))
                            .withMember("endpoint", ObjectNode.builder().withMember("url", url)
                                    .withMember("properties", Node.objectNode())
                                    .withMember("headers", Node.objectNode()).build()).build());
                }
            }
            return out;
        }

        /** A condition over what is in {@code scope}, which it may extend. */
        private Node condition(Scope scope) {
            switch (random.nextInt(6)) {
                case 0:
                    if (!scope.unproven.isEmpty()) {
                        String name = scope.unproven.remove(random.nextInt(scope.unproven.size()));
                        scope.strings.add(name);
                        return fn("isSet", ref(name));
                    }
                    break;
                case 1:
                    if (!scope.unproven.isEmpty()) {
                        return fn("not", fn("isSet", ref(pick(scope.unproven))));
                    }
                    break;
                case 2:
                    return fn("booleanEquals", ref("B" + random.nextInt(booleans)), Node.from(random.nextBoolean()));
                case 3:
                    break;
                default:
                    List<String> free = new ArrayList<>();
                    for (String name : NAMES) {
                        if (!scope.strings.contains(name)) {
                            free.add(name);
                        }
                    }
                    if (!free.isEmpty()) {
                        String name = free.get(random.nextInt(free.size()));
                        int start = random.nextInt(3);
                        Node value = fn("substring", ref(pick(scope.strings)), Node.from(start),
                                Node.from(start + 1 + random.nextInt(3)), Node.from(random.nextBoolean()));
                        scope.strings.add(name);
                        return value.expectObjectNode().withMember("assign", name);
                    }
                    break;
            }
            return fn("stringEquals", ref(pick(scope.strings)), Node.from("x" + random.nextInt(3)));
        }

        private String pick(List<String> names) {
            return names.get(random.nextInt(names.size()));
        }

        private static Node error(String message, List<Node> conditions) {
            return ObjectNode.builder().withMember("type", "error")
                    .withMember("conditions", ArrayNode.fromNodes(conditions))
                    .withMember("error", message).build();
        }

        private static Node ref(String name) {
            return ObjectNode.builder().withMember("ref", name).build();
        }

        private static Node fn(String name, Node... args) {
            return ObjectNode.builder().withMember("fn", name).withMember("argv", ArrayNode.fromNodes(args)).build();
        }
    }
}
