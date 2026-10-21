using System.Data.SQLite;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

[SQLiteFunction(Name = "UNICODE_NOCASE", FuncType = FunctionType.Collation)]
public sealed class UnicodeNoCaseCollation : SQLiteFunction
{
    public override int Compare(string? left, string? right)
    {
        if (left is null && right is null) return 0;
        if (left is null) return -1;
        if (right is null) return 1;
        return string.Compare(left, right, StringComparison.OrdinalIgnoreCase);
    }
}

internal static class Program
{
    private const int ExpectedSchemaVersion = 10;

    private static readonly string[] ExpectedServices =
    [
        "ArasakaDDNS",
        "ArasakaFFmpegWatchdog",
        "ArasakaFFmpegWorker",
        "CodexProxy",
        "MediaSTT",
        "OCI-A1-Grabber",
        "coord"
    ];

    private static readonly (string Left, string Right)[] CollationSamples =
    [
        ("Alpha", "aLPHA"),
        ("München", "MÜNCHEN"),
        ("Ä", "ä"),
        ("I", "i"),
        ("ı", "I"),
        ("Σ", "σ"),
        ("ß", "ẞ")
    ];

    private static int Main(string[] args)
    {
        try
        {
            SQLiteFunction.RegisterFunction(typeof(UnicodeNoCaseCollation));

            if (args.Length == 1 && args[0] == "--self-test")
            {
                RunSelfTest();
                return 0;
            }

            if (args.Length == 2 && args[0] == "--verify-snapshot")
            {
                var snapshot = Path.GetFullPath(args[1]);
                if (!File.Exists(snapshot))
                    throw new FileNotFoundException("snapshot does not exist", snapshot);
                var checkedSnapshot = ValidateSnapshot(snapshot);
                var info = new FileInfo(snapshot);
                Console.WriteLine(JsonSerializer.Serialize(new
                {
                    result = "PASS",
                    checkedUtc = DateTimeOffset.UtcNow,
                    access = "read-only",
                    snapshotPath = snapshot,
                    snapshotBytes = info.Length,
                    snapshotSha256 = Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(snapshot))),
                    runtime = RuntimeInformation.FrameworkDescription,
                    runtimeVersion = Environment.Version.ToString(),
                    providerAssemblyVersion = typeof(SQLiteConnection).Assembly.GetName().Version?.ToString(),
                    sqliteEngineVersion = SQLiteConnection.SQLiteVersion,
                    collation = "StringComparison.OrdinalIgnoreCase",
                    checkedSnapshot
                }));
                return 0;
            }

            if (args.Length != 4 || args[0] != "--source" || args[2] != "--snapshot")
            {
                Console.Error.WriteLine("usage: servy-snapshot-check.exe --source <Servy.db> --snapshot <new-snapshot.db> | --verify-snapshot <existing-snapshot.db> | --self-test");
                return 2;
            }

            var result = CreateSnapshotAndValidate(args[1], args[3]);
            Console.WriteLine(JsonSerializer.Serialize(result));
            return 0;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"error={ex.GetType().Name}: {ex.Message}");
            return 1;
        }
    }

    private static object CreateSnapshotAndValidate(string sourcePath, string snapshotPath)
    {
        var source = Path.GetFullPath(sourcePath);
        var snapshot = Path.GetFullPath(snapshotPath);

        if (string.Equals(source, snapshot, StringComparison.OrdinalIgnoreCase))
            throw new InvalidOperationException("source and snapshot paths must differ");
        if (!File.Exists(source))
            throw new FileNotFoundException("source database does not exist", source);
        if (File.Exists(snapshot))
            throw new IOException("snapshot already exists; refusing to overwrite it");
        if (!Directory.Exists(Path.GetDirectoryName(snapshot)))
            throw new DirectoryNotFoundException("snapshot parent directory does not exist");

        var sourceBuilder = new SQLiteConnectionStringBuilder
        {
            DataSource = source,
            Version = 3,
            ReadOnly = true,
            FailIfMissing = true,
            Pooling = false
        };

        using (var sourceConnection = new SQLiteConnection(sourceBuilder.ConnectionString))
        {
            sourceConnection.Open();
            using var createNew = new FileStream(snapshot, FileMode.CreateNew, FileAccess.Write, FileShare.None);
            createNew.Dispose();

            var destinationBuilder = new SQLiteConnectionStringBuilder
            {
                DataSource = snapshot,
                Version = 3,
                FailIfMissing = true,
                Pooling = false
            };

            using var destinationConnection = new SQLiteConnection(destinationBuilder.ConnectionString);
            destinationConnection.Open();
            sourceConnection.BackupDatabase(destinationConnection, "main", "main", -1, null, 0);
        }

        var checkedSnapshot = ValidateSnapshot(snapshot);
        var info = new FileInfo(snapshot);

        return new
        {
            result = "PASS",
            createdUtc = DateTimeOffset.UtcNow,
            sourcePath = source,
            sourceAccess = "read-only; SQLite Online Backup API",
            snapshotPath = snapshot,
            snapshotBytes = info.Length,
            snapshotSha256 = Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(snapshot))),
            runtime = RuntimeInformation.FrameworkDescription,
            runtimeVersion = Environment.Version.ToString(),
            providerAssemblyVersion = typeof(SQLiteConnection).Assembly.GetName().Version?.ToString(),
            sqliteEngineVersion = SQLiteConnection.SQLiteVersion,
            collation = "StringComparison.OrdinalIgnoreCase",
            checkedSnapshot
        };
    }

    private static object ValidateSnapshot(string snapshotPath)
    {
        var builder = new SQLiteConnectionStringBuilder
        {
            DataSource = snapshotPath,
            Version = 3,
            ReadOnly = true,
            FailIfMissing = true,
            Pooling = false
        };

        using var connection = new SQLiteConnection(builder.ConnectionString);
        connection.Open();
        Execute(connection, "PRAGMA query_only=ON;");

        var quickCheck = ReadPragmaResults(connection, "quick_check");
        var integrityCheck = ReadPragmaResults(connection, "integrity_check");
        RequireOk("quick_check", quickCheck);
        RequireOk("integrity_check", integrityCheck);

        VerifyCollation(connection);

        var index = VerifyServicesIndex(connection);
        var schema = VerifySchemaInfo(connection);
        var services = VerifyServices(connection);

        return new
        {
            quickCheck,
            integrityCheck,
            collationTests = CollationSamples.Length,
            index,
            schema,
            services
        };
    }

    private static void VerifyCollation(SQLiteConnection connection)
    {
        foreach (var (left, right) in CollationSamples)
        {
            using var command = connection.CreateCommand();
            command.CommandText = "SELECT CASE WHEN @left = @right COLLATE UNICODE_NOCASE THEN 0 WHEN @left COLLATE UNICODE_NOCASE < @right THEN -1 ELSE 1 END;";
            command.Parameters.AddWithValue("@left", left);
            command.Parameters.AddWithValue("@right", right);

            var sqliteOrder = Convert.ToInt32(command.ExecuteScalar(), CultureInfo.InvariantCulture);
            var dotnetOrder = Math.Sign(string.Compare(left, right, StringComparison.OrdinalIgnoreCase));
            if (sqliteOrder != dotnetOrder)
                throw new InvalidDataException("SQLite UNICODE_NOCASE does not match the Servy ordinal-ignore-case comparer");
        }
    }

    private static object VerifyServicesIndex(SQLiteConnection connection)
    {
        using var command = connection.CreateCommand();
        command.CommandText = "SELECT sql FROM sqlite_master WHERE type='index' AND name=@name;";
        command.Parameters.AddWithValue("@name", "idx_services_name_unique");
        var sql = command.ExecuteScalar() as string
            ?? throw new InvalidDataException("idx_services_name_unique is missing");

        if (!sql.Contains("UNIQUE", StringComparison.OrdinalIgnoreCase) ||
            !sql.Contains("Name COLLATE UNICODE_NOCASE", StringComparison.OrdinalIgnoreCase))
            throw new InvalidDataException("idx_services_name_unique does not match the expected unique Unicode collation definition");

        using var indexList = connection.CreateCommand();
        indexList.CommandText = "PRAGMA index_list('Services');";
        using var reader = indexList.ExecuteReader();
        var found = false;
        var unique = false;
        while (reader.Read())
        {
            if (!string.Equals(reader.GetString(1), "idx_services_name_unique", StringComparison.Ordinal))
                continue;
            found = true;
            unique = reader.GetInt32(2) == 1;
        }
        if (!found || !unique)
            throw new InvalidDataException("Services index metadata does not report idx_services_name_unique as unique");

        using var indexInfo = connection.CreateCommand();
        indexInfo.CommandText = "PRAGMA index_xinfo('idx_services_name_unique');";
        using var infoReader = indexInfo.ExecuteReader();
        var keyRows = new List<(string? Name, string Collation)>();
        while (infoReader.Read())
        {
            if (infoReader.GetInt32(5) != 1)
                continue;
            keyRows.Add((infoReader.IsDBNull(2) ? null : infoReader.GetString(2), infoReader.GetString(4)));
        }

        if (keyRows.Count != 1 || keyRows[0].Name != "Name" ||
            !string.Equals(keyRows[0].Collation, "UNICODE_NOCASE", StringComparison.Ordinal))
            throw new InvalidDataException("idx_services_name_unique key metadata is not Name COLLATE UNICODE_NOCASE");

        return new { unique, key = keyRows[0].Name, collation = keyRows[0].Collation };
    }

    private static object VerifySchemaInfo(SQLiteConnection connection)
    {
        using var command = connection.CreateCommand();
        command.CommandText = "SELECT Id, Version FROM SchemaInfo ORDER BY Id;";
        using var reader = command.ExecuteReader();
        if (!reader.Read())
            throw new InvalidDataException("SchemaInfo has no row");

        var id = reader.GetInt32(0);
        var version = reader.GetInt32(1);
        if (reader.Read())
            throw new InvalidDataException("SchemaInfo has more than one row");
        if (id != 1 || version != ExpectedSchemaVersion)
            throw new InvalidDataException($"SchemaInfo is unexpected (Id={id}, Version={version})");

        return new { rows = 1, id, version };
    }

    private static object VerifyServices(SQLiteConnection connection)
    {
        using var command = connection.CreateCommand();
        command.CommandText = "SELECT * FROM Services ORDER BY Name COLLATE UNICODE_NOCASE;";
        using var reader = command.ExecuteReader();

        var names = new List<string>();
        var hashes = new List<string>();
        while (reader.Read())
        {
            var nameOrdinal = FindOrdinal(reader, "Name");
            var name = reader.GetString(nameOrdinal);
            names.Add(name);
            hashes.Add(HashRow(reader));
        }

        if (names.Count != ExpectedServices.Length ||
            !names.OrderBy(name => name, StringComparer.Ordinal).SequenceEqual(ExpectedServices.OrderBy(name => name, StringComparer.Ordinal), StringComparer.Ordinal))
            throw new InvalidDataException("Services rows do not match the expected seven managed service names");

        return new
        {
            rows = names.Count,
            serviceNames = names,
            recordSha256 = hashes
        };
    }

    private static int FindOrdinal(SQLiteDataReader reader, string name)
    {
        for (var i = 0; i < reader.FieldCount; i++)
        {
            if (string.Equals(reader.GetName(i), name, StringComparison.OrdinalIgnoreCase))
                return i;
        }
        throw new InvalidDataException($"Services table has no {name} column");
    }

    private static string HashRow(SQLiteDataReader reader)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        for (var i = 0; i < reader.FieldCount; i++)
        {
            AppendHashBytes(hash, Encoding.UTF8.GetBytes(reader.GetName(i)));
            var value = reader.GetValue(i);
            if (value is DBNull)
            {
                hash.AppendData([0]);
                continue;
            }

            if (value is byte[] bytes)
            {
                hash.AppendData([1]);
                AppendHashBytes(hash, bytes);
            }
            else
            {
                hash.AppendData([2]);
                AppendHashBytes(hash, Encoding.UTF8.GetBytes(Convert.ToString(value, CultureInfo.InvariantCulture) ?? ""));
            }
        }
        return Convert.ToHexString(hash.GetHashAndReset());
    }

    private static void AppendHashBytes(IncrementalHash hash, byte[] bytes)
    {
        hash.AppendData(BitConverter.GetBytes(bytes.Length));
        hash.AppendData(bytes);
    }

    private static List<string> ReadPragmaResults(SQLiteConnection connection, string pragma)
    {
        using var command = connection.CreateCommand();
        command.CommandText = $"PRAGMA {pragma};";
        using var reader = command.ExecuteReader();
        var results = new List<string>();
        while (reader.Read())
            results.Add(reader.GetString(0));
        return results;
    }

    private static void RequireOk(string name, IReadOnlyList<string> results)
    {
        if (results.Count != 1 || !string.Equals(results[0], "ok", StringComparison.OrdinalIgnoreCase))
            throw new InvalidDataException($"PRAGMA {name} failed: {string.Join(";", results)}");
    }

    private static void Execute(SQLiteConnection connection, string sql)
    {
        using var command = connection.CreateCommand();
        command.CommandText = sql;
        command.ExecuteNonQuery();
    }

    private static void RunSelfTest()
    {
        var directory = Path.Combine(Path.GetTempPath(), "servy-snapshot-selftest-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(directory);
        try
        {
            var source = Path.Combine(directory, "source.db");
            var snapshot = Path.Combine(directory, "snapshot.db");
            var builder = new SQLiteConnectionStringBuilder
            {
                DataSource = source,
                Version = 3,
                Pooling = false
            };

            using (var connection = new SQLiteConnection(builder.ConnectionString))
            {
                connection.Open();
                Execute(connection, "PRAGMA journal_mode=WAL;");
                Execute(connection, "CREATE TABLE SchemaInfo (Id INTEGER PRIMARY KEY, Version INTEGER NOT NULL);");
                Execute(connection, "INSERT INTO SchemaInfo (Id, Version) VALUES (1, 10);");
                Execute(connection, "CREATE TABLE Services (Id TEXT PRIMARY KEY, Name TEXT NOT NULL, Config TEXT NOT NULL);");
                Execute(connection, "CREATE UNIQUE INDEX idx_services_name_unique ON Services(Name COLLATE UNICODE_NOCASE);");
                foreach (var name in ExpectedServices)
                {
                    using var insert = connection.CreateCommand();
                    insert.CommandText = "INSERT INTO Services (Id, Name, Config) VALUES (@id, @name, @config);";
                    insert.Parameters.AddWithValue("@id", Guid.NewGuid().ToString("D"));
                    insert.Parameters.AddWithValue("@name", name);
                    insert.Parameters.AddWithValue("@config", "self-test-only");
                    insert.ExecuteNonQuery();
                }
            }

            _ = CreateSnapshotAndValidate(source, snapshot);
            var snapshotHash = Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(snapshot)));
            try
            {
                _ = CreateSnapshotAndValidate(source, snapshot);
                throw new InvalidOperationException("self-test failed: existing snapshot was overwritten");
            }
            catch (IOException ex) when (ex.Message.Contains("refusing to overwrite", StringComparison.Ordinal))
            {
            }
            if (snapshotHash != Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(snapshot))))
                throw new InvalidDataException("self-test failed: existing snapshot content changed");

            Console.WriteLine("self_test=PASS backup=SQLite Online Backup API read_only_checks=PASS collation=PASS overwrite_protection=PASS fixture=temporary");
        }
        finally
        {
            Directory.Delete(directory, recursive: true);
        }
    }
}
