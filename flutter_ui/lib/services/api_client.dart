import 'package:dio/dio.dart';
import '../models/job.dart';
import '../models/skill.dart';
import '../models/cron_schedule.dart';

class ApiClient {
  final Dio _dio;

  ApiClient(String baseUrl)
      : _dio = Dio(BaseOptions(
          baseUrl: '$baseUrl/api/v1',
          connectTimeout: const Duration(seconds: 10),
          receiveTimeout: const Duration(seconds: 30),
        ));

  // Jobs
  Future<Map<String, dynamic>> submitJob({
    required String prompt,
    List<String> skillIds = const [],
    List<String> skillTags = const [],
    String? model,
    double? maxBudget,
    int? priority,
    List<String> tags = const [],
    String? workingDir,
    int? timeoutSecs,
    Map<String, dynamic>? outputDest,
    List<String>? allowedTools,
  }) async {
    final resp = await _dio.post('/jobs', data: {
      'prompt': prompt,
      if (skillIds.isNotEmpty) 'skill_ids': skillIds,
      if (skillTags.isNotEmpty) 'skill_tags': skillTags,
      if (model != null) 'model': model,
      if (maxBudget != null) 'max_budget_usd': maxBudget,
      if (priority != null) 'priority': priority,
      if (tags.isNotEmpty) 'tags': tags,
      if (workingDir != null) 'working_dir': workingDir,
      if (timeoutSecs != null) 'timeout_secs': timeoutSecs,
      if (outputDest != null) 'output_dest': outputDest,
      if (allowedTools != null && allowedTools.isNotEmpty)
        'allowed_tools': allowedTools,
    });
    return resp.data;
  }

  Future<List<Job>> listJobs({String? status, int limit = 20}) async {
    final params = <String, dynamic>{'limit': limit};
    if (status != null) params['status'] = status;
    final resp = await _dio.get('/jobs', queryParameters: params);
    final items = resp.data['items'] as List? ?? [];
    return items.map((j) => Job.fromJson(j)).toList();
  }

  Future<Job> getJob(String id) async {
    final resp = await _dio.get('/jobs/$id');
    return Job.fromJson(resp.data);
  }

  Future<JobResult> getResult(String id) async {
    final resp = await _dio.get('/jobs/$id/result');
    return JobResult.fromJson(resp.data);
  }

  Future<List<String>> getLogs(String id) async {
    final resp = await _dio.get('/jobs/$id/logs');
    return List<String>.from(resp.data['lines'] ?? []);
  }

  Future<void> cancelJob(String id) async {
    await _dio.post('/jobs/$id/cancel');
  }

  Future<void> deleteJob(String id) async {
    await _dio.delete('/jobs/$id');
  }

  Future<QueueStatus> getStatus() async {
    final resp = await _dio.get('/status');
    return QueueStatus.fromJson(resp.data['queue']);
  }

  // Skills
  Future<List<Skill>> listSkills() async {
    final resp = await _dio.get('/skills');
    final items = resp.data['items'] as List? ?? [];
    return items.map((s) => Skill.fromJson(s)).toList();
  }

  Future<Skill> getSkill(String id) async {
    final resp = await _dio.get('/skills/$id');
    return Skill.fromJson(resp.data);
  }

  Future<void> createSkill(Skill skill) async {
    await _dio.post('/skills', data: skill.toJson());
  }

  Future<void> updateSkill(String id, Skill skill) async {
    await _dio.put('/skills/$id', data: skill.toJson());
  }

  Future<void> deleteSkill(String id) async {
    await _dio.delete('/skills/$id');
  }

  // Crons
  Future<List<CronSchedule>> listCrons() async {
    final resp = await _dio.get('/crons');
    final items = resp.data['items'] as List? ?? [];
    return items.map((c) => CronSchedule.fromJson(c)).toList();
  }

  Future<CronSchedule> getCron(String id) async {
    final resp = await _dio.get('/crons/$id');
    return CronSchedule.fromJson(resp.data);
  }

  Future<CronSchedule> createCron(Map<String, dynamic> data) async {
    final resp = await _dio.post('/crons', data: data);
    return CronSchedule.fromJson(resp.data);
  }

  Future<CronSchedule> updateCron(String id, Map<String, dynamic> data) async {
    final resp = await _dio.put('/crons/$id', data: data);
    return CronSchedule.fromJson(resp.data);
  }

  Future<void> deleteCron(String id) async {
    await _dio.delete('/crons/$id');
  }

  Future<Map<String, dynamic>> triggerCron(String id) async {
    final resp = await _dio.post('/crons/$id/trigger');
    return resp.data;
  }
}
