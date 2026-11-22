using BenchmarkDotNet.Running;

BenchmarkSwitcher
    .FromAssembly(typeof(StatusBenchmarks).Assembly)
    .Run(args);
