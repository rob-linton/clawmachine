import 'package:dio/dio.dart';
import '../models/job.dart';
import '../models/skill.dart';

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
    String? model,
    double? maxBudget,
    int? priority,
    List<String> tags = const [],
  }) async {
    final resp = await _dio.post('/jobs', data: {
      'prompt': prompt,
      if (skillIds.isNotEmpty) 'skill_ids': skillIds,
      if (model != null) 'model': model,
      if (maxBudget != null) 'max_budget_usd': maxBudget,
      if (priority != null) 'priority': priority,
      if (tags.isNotEmpty) 'tags': tags,
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

  Future<void> deleteSkill(String id) async {
    await _dio.delete('/skills/$id');
  }
}
