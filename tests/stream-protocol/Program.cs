namespace Zipelo.StreamProtocolTests;

internal static class Program
{
    private static readonly (string Name, Func<Task> Run)[] Cases =
    [
        ("wire.empty_job", () => ChannelTests.RoundTrip([])),
        ("wire.empty_item", () => ChannelTests.RoundTrip([0])),
        ("wire.one_byte", () => ChannelTests.RoundTrip([1])),
        ("wire.chunk_boundaries", () => ChannelTests.RoundTrip([65535, 65536])),
        ("wire.fragmented_reads", () => ChannelTests.RoundTrip([1, 7, 1024], 3)),
        ("wire.golden_64_byte_header", () => ChannelTests.GoldenHeader()),
        ("wire.all_header_truncations", () => ChannelTests.TruncatedHeaders()),
        ("wire.metadata_and_data_truncations", () => ChannelTests.TruncatedBodies()),
        ("wire.magic_version_reserved", () => ChannelTests.FixedFields()),
        ("wire.kind_length_and_uint_overflow", () => ChannelTests.KindsAndLengths()),
        ("wire.huge_declaration_allocation_bound", () => ChannelTests.HugeDeclarations()),
        ("state.job_and_item_ids", () => ValidatorTests.Identifiers()),
        ("state.sequence_duplicate_gap_overflow", () => ValidatorTests.Sequences()),
        ("state.missing_and_nested_transitions", () => ValidatorTests.Transitions()),
        ("state.duplicate_item_id", () => ValidatorTests.DuplicateItem()),
        ("state.actual_declared_end_lengths", () => ValidatorTests.ItemLengths()),
        ("state.status_and_open_item", () => ValidatorTests.TerminalStatus()),
        ("state.failed_cancelled_eof", () => ChannelTests.FailedTerminals()),
        ("state.eof_before_terminal", () => ChannelTests.EarlyEof()),
        ("state.trailing_byte_duplicate_end", () => ChannelTests.TrailingData()),
        ("state.terminal_without_eof_deadline", () => ChannelTests.MissingEof()),
        ("state.final_status_over_cancel_and_deadline", () => ChannelTests.FinalStatePriority()),
        ("budget.invalid_limits", () => ValidatorTests.Limits()),
        ("budget.validate_header_does_not_spend", () => ValidatorTests.HeaderValidationDoesNotSpend()),
        ("budget.wire_exact_and_one_short", () => ValidatorTests.WireBudget()),
        ("budget.data_cumulative_16mib", () => ValidatorTests.DataBudget()),
        ("budget.small_chunk_frame_count", () => ValidatorTests.FrameBudget()),
        ("budget.empty_item_count", () => ValidatorTests.ItemBudget()),
        ("budget.signed_and_payload_lengths", () => ValidatorTests.ArithmeticLengths()),
        ("cancel.before_io", () => ChannelTests.PreCancelled()),
        ("cancel.read_header", () => ChannelTests.BlockedRead(false, false)),
        ("cancel.read_body", () => ChannelTests.BlockedRead(true, false)),
        ("cancel.write_header", () => ChannelTests.BlockedWrite("header", false)),
        ("cancel.write_body", () => ChannelTests.BlockedWrite("body", false)),
        ("cancel.write_flush", () => ChannelTests.BlockedWrite("flush", false)),
        ("deadline.read_wait", () => ChannelTests.BlockedRead(true, true)),
        ("deadline.write_wait", () => ChannelTests.BlockedWrite("body", true)),
        ("deadline.between_calls", () => ChannelTests.BetweenCallsDeadline()),
        ("flow.slow_producer", () => ChannelTests.BlockedRead(true, false, true)),
        ("flow.slow_consumer_snapshot", () => ChannelTests.BlockedWrite("body", false, true)),
        ("flow.flush_completion", () => ChannelTests.BlockedWrite("flush", false, true)),
        ("fault.read_io_and_reuse", () => ChannelTests.IoFailure(true)),
        ("fault.write_io_and_reuse", () => ChannelTests.IoFailure(false)),
        ("api.concurrent_call_and_dispose", () => ChannelTests.ConcurrentCalls()),
        ("api.direction_stream_lifetime", () => ChannelTests.DirectionAndLifetime()),
        ("api.invalid_write_before_output", () => ChannelTests.InvalidWrite()),
    ];

    public static async Task<int> Main(string[] arguments)
    {
        if (arguments is ["--list"])
        {
            foreach (var item in Cases) { Console.WriteLine(item.Name); }
            Console.WriteLine($"등록: {Cases.Length}");
            return 0;
        }
        if (arguments.Length != 0) { return 2; }
        var passed = 0;
        var failed = 0;
        foreach (var (name, run) in Cases)
        {
            try
            {
                await run().WaitAsync(TimeSpan.FromSeconds(8));
                Console.WriteLine($"PASS {name}");
                passed++;
            }
            catch (TimeoutException)
            {
                Console.Error.WriteLine($"FAIL {name}: 검사 제한 시간 초과");
                Console.WriteLine($"등록 {Cases.Length}, 통과 {passed}, 실패 {failed + 1}, 미실행 {Cases.Length - passed - failed - 1}");
                return 124;
            }
            catch (Exception exception)
            {
                Console.Error.WriteLine($"FAIL {name}: {exception.GetType().Name}: {exception.Message}");
                failed++;
            }
        }
        Console.WriteLine($"등록 {Cases.Length}, 통과 {passed}, 실패 {failed}, SKIP 0");
        return failed == 0 ? 0 : 1;
    }
}
