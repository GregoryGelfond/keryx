email(notices(r0, 0), "ops@example.com").
notice(notices(r0, 0)).
notice(notices(r0, 1)).
outbox(r0).
sms(notices(r0, 1), "+15551234").
subject(notices(r0, 0), "Deploy complete").
subject(notices(r0, 1), "Disk 90% full").
